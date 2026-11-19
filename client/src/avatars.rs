use crate::appearance::portable_path;
use crate::artwork::decode_image;
use crate::model::AvatarId;
use eframe::egui;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::UNIX_EPOCH;

const PLACEHOLDERS: AvatarId = 12;
const MAX_FILE_BYTES: u64 = 3_000_000;
const MAX_TEXTURES: usize = 160;

#[derive(Clone, PartialEq, Eq)]
struct FileEntry {
    path: PathBuf,
    len: u64,
    modified_ns: u128,
}

struct DecodeRequest {
    generation: u64,
    id: AvatarId,
    paths: Vec<PathBuf>,
    ctx: egui::Context,
}

struct DecodeResult {
    generation: u64,
    id: AvatarId,
    image: Option<egui::ColorImage>,
}

struct LoaderState {
    textures: VecDeque<(AvatarId, Option<egui::TextureHandle>)>,
    pending: HashSet<AvatarId>,
    req_tx: Option<Sender<DecodeRequest>>,
    resp_rx: Option<Receiver<DecodeResult>>,
}

impl LoaderState {
    fn new() -> Self {
        Self {
            textures: VecDeque::new(),
            pending: HashSet::new(),
            req_tx: None,
            resp_rx: None,
        }
    }

    fn ensure_worker(&mut self) -> &Sender<DecodeRequest> {
        if self.req_tx.is_none() {
            let (req_tx, req_rx) = mpsc::channel::<DecodeRequest>();
            let (resp_tx, resp_rx) = mpsc::channel::<DecodeResult>();
            std::thread::spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    let image = req.paths.iter().find_map(|path| decode_avatar_image(path));
                    let _ = resp_tx.send(DecodeResult {
                        generation: req.generation,
                        id: req.id,
                        image,
                    });
                    req.ctx.request_repaint();
                }
            });
            self.req_tx = Some(req_tx);
            self.resp_rx = Some(resp_rx);
        }
        self.req_tx.as_ref().expect("avatar worker sender")
    }
}

pub struct Avatars {
    directories: Vec<PathBuf>,
    ids: Vec<AvatarId>,
    files: BTreeMap<AvatarId, Vec<FileEntry>>,
    generation: u64,
    loader: RefCell<LoaderState>,
}

impl Avatars {
    pub fn discover() -> Self {
        let portable = portable_path("avatars");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/avatars");
        Self::from_directories(&[&portable, &source])
    }

    fn from_directories(directories: &[&Path]) -> Self {
        let dirs: Vec<PathBuf> = directories.iter().map(|p| p.to_path_buf()).collect();
        let (ids, files) = scan_directories(&dirs);
        Self {
            directories: dirs,
            ids,
            files,
            generation: 0,
            loader: RefCell::new(LoaderState::new()),
        }
    }

    /// Rescans avatar folders while preserving already-loaded GPU textures for unchanged files.
    /// Pass `force_reload = true` (e.g. on Ctrl+Shift+R) to discard all cached textures.
    pub fn rescan(&mut self, force_reload: bool) {
        let (ids, new_files) = scan_directories(&self.directories);
        let mut loader = self.loader.borrow_mut();
        if force_reload {
            loader.textures.clear();
            loader.pending.clear();
            self.generation = self.generation.wrapping_add(1);
        } else {
            let mut changed = false;
            loader.textures.retain(|(id, _)| {
                let keep = self.files.get(id) == new_files.get(id) && new_files.contains_key(id);
                if !keep {
                    changed = true;
                }
                keep
            });
            loader.pending.retain(|id| {
                let keep = self.files.get(id) == new_files.get(id) && new_files.contains_key(id);
                if !keep {
                    changed = true;
                }
                keep
            });
            if changed {
                self.generation = self.generation.wrapping_add(1);
            }
        }
        self.ids = ids;
        self.files = new_files;
    }

    pub fn ids(&self) -> &[AvatarId] {
        &self.ids
    }

    /// Returns a cached GPU texture immediately, or schedules non-blocking background decoding
    /// so opening the avatar picker never stalls the UI thread.
    pub fn texture(&self, ctx: &egui::Context, id: AvatarId) -> Option<egui::TextureHandle> {
        let mut loader = self.loader.borrow_mut();
        self.drain_ready(ctx, &mut loader);

        if let Some(index) = loader.textures.iter().position(|(key, _)| *key == id) {
            let entry = loader.textures.remove(index).expect("cached avatar");
            let texture = entry.1.clone();
            loader.textures.push_back(entry);
            return texture;
        }

        let entries = self.files.get(&id)?;
        if !loader.pending.contains(&id) {
            loader.pending.insert(id);
            let paths = entries.iter().map(|e| e.path.clone()).collect();
            let generation = self.generation;
            let _ = loader.ensure_worker().send(DecodeRequest {
                generation,
                id,
                paths,
                ctx: ctx.clone(),
            });
        }
        None
    }

    fn drain_ready(&self, ctx: &egui::Context, loader: &mut LoaderState) {
        let mut ready = Vec::new();
        if let Some(rx) = loader.resp_rx.as_ref() {
            while let Ok(item) = rx.try_recv() {
                ready.push(item);
            }
        }
        for item in ready {
            if item.generation != self.generation {
                continue;
            }
            loader.pending.remove(&item.id);
            let handle = item.image.map(|color_image| {
                ctx.load_texture(
                    format!("avatar-{}", item.id),
                    color_image,
                    egui::TextureOptions::LINEAR
                        .with_mipmap_mode(Some(egui::TextureFilter::Linear)),
                )
            });
            if let Some(pos) = loader.textures.iter().position(|(k, _)| *k == item.id) {
                loader.textures.remove(pos);
            }
            loader.textures.push_back((item.id, handle));
            while loader.textures.len() > MAX_TEXTURES {
                loader.textures.pop_front();
            }
        }
    }
}

fn scan_directories(directories: &[PathBuf]) -> (Vec<AvatarId>, BTreeMap<AvatarId, Vec<FileEntry>>) {
    let mut files = BTreeMap::<AvatarId, Vec<FileEntry>>::new();
    for directory in directories {
        let mut candidates = fs::read_dir(directory)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| {
                let path = entry.path();
                let (id, extension) = numbered_image(&path)?;
                let meta = entry.metadata().ok();
                let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified_ns = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                Some((id, extension, path, len, modified_ns))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        for (id, _, path, len, modified_ns) in candidates {
            files.entry(id).or_default().push(FileEntry {
                path,
                len,
                modified_ns,
            });
        }
    }
    let ids = (1..=PLACEHOLDERS)
        .chain(files.range((PLACEHOLDERS + 1)..).map(|(&id, _)| id))
        .collect();
    (ids, files)
}

fn numbered_image(path: &Path) -> Option<(AvatarId, usize)> {
    let extension = path.extension()?.to_str()?;
    let priority = ["png", "jpg", "jpeg", "webp"]
        .iter()
        .position(|format| extension.eq_ignore_ascii_case(format))?;
    let stem = path.file_stem()?.to_str()?;
    if !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let id = stem.parse::<AvatarId>().ok()?;
    (id > 0).then_some((id, priority))
}

fn decode_avatar_image(path: &Path) -> Option<egui::ColorImage> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return None;
    }
    let image = decode_image(&bytes, 1024)?;
    let image = if image.width().max(image.height()) > 256 {
        image.resize(256, 256, image::imageops::FilterType::Triangle)
    } else {
        image
    }
    .into_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_texture(avatars: &Avatars, ctx: &egui::Context, id: AvatarId) -> egui::TextureHandle {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(tex) = avatars.texture(ctx, id) {
                return tex;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for background avatar decode"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn accepts_sparse_avatar_numbers_beyond_the_original_picker() {
        for (name, expected) in [
            ("01.jpg", Some((1, 1))),
            ("13.PNG", Some((13, 0))),
            ("40000.webp", Some((40000, 3))),
            ("4294967295.jpeg", Some((u32::MAX, 2))),
            ("4294967296.png", None),
            ("0.png", None),
            ("-13.png", None),
            ("avatar.png", None),
            ("123.gif", None),
        ] {
            assert_eq!(numbered_image(Path::new(name)), expected, "{name}");
        }
    }

    #[test]
    fn discovers_and_loads_a_high_numbered_avatar_after_rescan() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!("murr-avatars-{unique}"));
        fs::create_dir(&folder).unwrap();
        let mut avatars = Avatars::from_directories(&[&folder]);
        assert_eq!(avatars.ids().len(), 12);

        let file = folder.join("40000.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .save(&file)
            .unwrap();
        avatars.rescan(false);
        let ctx = egui::Context::default();
        let texture = wait_for_texture(&avatars, &ctx, 40000);
        assert_eq!(avatars.ids().len(), 13);
        assert_eq!(avatars.ids().last(), Some(&40000));
        assert_eq!(texture.size(), [2, 2]);

        // Unchanged avatar must remain immediately cached after another rescan.
        avatars.rescan(false);
        assert!(avatars.texture(&ctx, 40000).is_some());

        fs::remove_file(&file).unwrap();
        fs::remove_dir(&folder).unwrap();
    }
}
