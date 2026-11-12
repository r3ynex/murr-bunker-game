use crate::appearance::portable_path;
use crate::artwork::decode_image;
use crate::model::AvatarId;
use eframe::egui;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const PLACEHOLDERS: AvatarId = 12;
const MAX_FILE_BYTES: u64 = 3_000_000;
const MAX_TEXTURES: usize = 160;

pub struct Avatars {
    ids: Vec<AvatarId>,
    files: BTreeMap<AvatarId, Vec<PathBuf>>,
    textures: RefCell<VecDeque<(AvatarId, Option<egui::TextureHandle>)>>,
}

impl Avatars {
    pub fn discover() -> Self {
        let portable = portable_path("avatars");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/avatars");
        Self::from_directories(&[&portable, &source])
    }

    fn from_directories(directories: &[&Path]) -> Self {
        let mut files = BTreeMap::<_, Vec<_>>::new();
        for directory in directories {
            let mut candidates = fs::read_dir(directory)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| {
                    let path = entry.path();
                    let (id, extension) = numbered_image(&path)?;
                    Some((id, extension, path))
                })
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            for (id, _, path) in candidates {
                files.entry(id).or_default().push(path);
            }
        }
        let ids = (1..=PLACEHOLDERS)
            .chain(files.range((PLACEHOLDERS + 1)..).map(|(&id, _)| id))
            .collect();
        Self {
            ids,
            files,
            textures: RefCell::new(VecDeque::new()),
        }
    }

    pub fn ids(&self) -> &[AvatarId] {
        &self.ids
    }

    pub fn texture(&self, ctx: &egui::Context, id: AvatarId) -> Option<egui::TextureHandle> {
        let paths = self.files.get(&id)?;
        let mut textures = self.textures.borrow_mut();
        let cached = textures.iter().position(|(key, _)| *key == id);
        let texture = match cached {
            Some(index) => textures.remove(index).expect("cached avatar").1,
            None => paths.iter().find_map(|path| load_texture(ctx, id, path)),
        };
        textures.push_back((id, texture.clone()));
        if textures.len() > MAX_TEXTURES {
            textures.pop_front();
        }
        texture
    }
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

fn load_texture(ctx: &egui::Context, id: AvatarId, path: &Path) -> Option<egui::TextureHandle> {
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
        image.resize(256, 256, image::imageops::FilterType::Lanczos3)
    } else {
        image
    }
    .into_rgba8();
    Some(ctx.load_texture(
        format!("avatar-{id}"),
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let before = Avatars::from_directories(&[&folder]);

        let file = folder.join("40000.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .save(&file)
            .unwrap();
        let after = Avatars::from_directories(&[&folder]);
        let texture = after.texture(&egui::Context::default(), 40000);
        fs::remove_file(&file).unwrap();
        fs::remove_dir(&folder).unwrap();

        assert_eq!(before.ids().len(), 12);
        assert_eq!(after.ids().len(), 13);
        assert_eq!(after.ids().last(), Some(&40000));
        assert_eq!(texture.unwrap().size(), [2, 2]);
    }
}
