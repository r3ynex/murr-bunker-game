use eframe::egui;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_REPO: &str = "r3ynex/murr-bunker-game";
const ASSET_NAME: &str = "Murr-Bunker.exe";
const MAX_EXE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
    state: String,
    digest: Option<String>,
}

struct Update {
    version: Version,
    bytes: Vec<u8>,
}

enum State {
    Idle,
    Checking {
        reply: Receiver<Result<Option<Update>, String>>,
        manual: bool,
    },
    Ready(Update),
    Restarting,
}

pub struct Updater {
    state: State,
    startup_notice: Option<&'static str>,
}

impl Updater {
    pub fn new() -> Self {
        let mut updater = Self {
            state: State::Idle,
            startup_notice: match std::env::args_os().nth(1).as_deref() {
                Some(arg) if arg == "--murr-update-installed" => Some("Игра обновлена"),
                Some(arg) if arg == "--murr-update-failed" => {
                    Some("Не удалось установить обновление. Проверь, закрыты ли другие окна игры.")
                }
                _ => None,
            },
        };
        if !cfg!(debug_assertions) && updater.startup_notice.is_none() {
            updater.check(false);
        }
        updater
    }

    pub fn can_check(&self) -> bool {
        cfg!(windows) && !cfg!(debug_assertions) && matches!(self.state, State::Idle)
    }

    pub fn check(&mut self, manual: bool) {
        if self.can_check() {
            let (sender, reply) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(download_latest());
            });
            self.state = State::Checking { reply, manual };
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context, can_install: bool) -> Option<String> {
        if let Some(notice) = self.startup_notice.take() {
            return Some(notice.into());
        }
        if let State::Checking { reply, manual } = &self.state {
            let manual = *manual;
            let result = match reply.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(150));
                    return None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("Проверка обновлений прервалась".into())
                }
            };
            self.state = State::Idle;
            match result {
                Ok(Some(update)) => {
                    self.state = State::Ready(update);
                    if !can_install {
                        return Some(
                            "Обновление скачано и установится после возвращения в меню".into(),
                        );
                    }
                }
                Ok(None) if manual => return Some("Установлена актуальная версия".into()),
                Err(error) if manual => return Some(error),
                _ => return None,
            }
        }
        if let State::Ready(update) = &self.state {
            if can_install {
                let version = update.version.clone();
                return match install(update) {
                    Ok(()) => {
                        self.state = State::Restarting;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        Some(format!("Устанавливается версия {version}…"))
                    }
                    Err(error) => {
                        self.state = State::Idle;
                        Some(format!("Не удалось подготовить обновление: {error}"))
                    }
                };
            }
        }
        None
    }
}

fn release_repository() -> Result<&'static str, String> {
    let repo = option_env!("MURR_RELEASE_REPO")
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_REPO);
    let valid_part = |part: &str| {
        !part.is_empty()
            && !matches!(part, "." | "..")
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    };
    if !repo
        .split_once('/')
        .is_some_and(|(owner, name)| valid_part(owner) && valid_part(name))
    {
        return Err("Некорректное имя репозитория обновлений".into());
    }
    Ok(repo)
}

fn asset_for_release<'a>(
    release: &'a Release,
    installed: &Version,
) -> Result<Option<(Version, &'a Asset)>, String> {
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .and_then(|tag| Version::parse(tag).ok())
        .ok_or("Тег релиза должен иметь вид v1.2.3")?;
    if !version.pre.is_empty() || version.cmp_precedence(installed).is_le() {
        return Ok(None);
    }
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == ASSET_NAME && asset.state == "uploaded")
        .ok_or("В релизе нет Windows EXE")?;
    if !(2..=MAX_EXE_BYTES).contains(&asset.size) {
        return Err("Размер файла обновления недопустим".into());
    }
    Ok(Some((version, asset)))
}

fn api_request(agent: &ureq::Agent, url: &str, accept: &str, token: Option<&str>) -> ureq::Request {
    let request = agent
        .get(url)
        .set(
            "User-Agent",
            concat!("Murr-Bunker/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", accept)
        .set("X-GitHub-Api-Version", "2022-11-28");
    match token {
        Some(token) => request.set("Authorization", &format!("Bearer {token}")),
        None => request,
    }
}

fn download_latest() -> Result<Option<Update>, String> {
    let repo = release_repository()?;
    let api = format!("https://api.github.com/repos/{repo}/releases");
    let token = std::env::var("MURR_UPDATE_TOKEN").ok();
    let token = token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_agent = ureq::AgentBuilder::new()
        .https_only(true)
        .redirects(0)
        .timeout(Duration::from_secs(12))
        .build();
    let response = match api_request(
        &api_agent,
        &format!("{api}/latest"),
        "application/vnd.github+json",
        token,
    )
    .call()
    {
        Ok(response) if response.status() == 200 => response,
        Err(ureq::Error::Status(404, _)) => {
            return Err("Стабильный релиз недоступен. Для приватного репозитория проверь MURR_UPDATE_TOKEN.".into());
        }
        _ => return Err("Не удалось проверить релизы GitHub".into()),
    };
    let mut json = Vec::new();
    response
        .into_reader()
        .take(1_048_577)
        .read_to_end(&mut json)
        .map_err(|_| "Не удалось прочитать данные релиза")?;
    if json.len() > 1_048_576 {
        return Err("Ответ GitHub слишком большой".into());
    }
    let release: Release =
        serde_json::from_slice(&json).map_err(|_| "Неверные данные релиза GitHub")?;
    let installed = Version::parse(VERSION).expect("Cargo package version");
    let Some((version, asset)) = asset_for_release(&release, &installed)? else {
        return Ok(None);
    };
    let digest = asset
        .digest
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("GitHub не предоставил SHA-256 для файла обновления")?;
    let download = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout(Duration::from_secs(90))
        .build();
    let response = if token.is_some() {
        let response = api_request(
            &api_agent,
            &format!("{api}/assets/{}", asset.id),
            "application/octet-stream",
            token,
        )
        .timeout(Duration::from_secs(90))
        .call()
        .map_err(|_| "Не удалось скачать EXE из GitHub Releases")?;
        if response.status() == 302 {
            let url = response
                .header("Location")
                .filter(|url| {
                    url.starts_with("https://release-assets.githubusercontent.com/")
                        || url.starts_with("https://objects.githubusercontent.com/")
                })
                .ok_or("GitHub вернул неизвестный адрес файла обновления")?;
            download
                .get(url)
                .call()
                .map_err(|_| "Не удалось скачать EXE из GitHub Releases")?
        } else {
            response
        }
    } else {
        let url = format!(
            "https://github.com/{repo}/releases/download/{}/{ASSET_NAME}",
            release.tag_name
        );
        download
            .get(&url)
            .call()
            .map_err(|_| "Не удалось скачать EXE из GitHub Releases")?
    };
    if response.status() != 200 {
        return Err("GitHub не вернул файл обновления".into());
    }
    let bytes = verify_exe(response.into_reader(), asset.size, digest)?;
    Ok(Some(Update { version, bytes }))
}

fn verify_exe(reader: impl Read, size: u64, digest: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(size + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Загрузка обновления прервалась")?;
    if bytes.len() as u64 != size || !bytes.starts_with(b"MZ") {
        return Err("Скачан неполный или неверный EXE".into());
    }
    if !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(digest) {
        return Err("SHA-256 скачанного EXE не совпадает с релизом".into());
    }
    Ok(bytes)
}

#[cfg(windows)]
fn install(update: &Update) -> std::io::Result<()> {
    use std::fs::{self, File};
    use std::io::{self, Write};
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    let target = std::env::current_exe()?;
    let directory = target
        .parent()
        .ok_or_else(|| io::Error::other("Папка игры не найдена"))?;
    let current_hash = format!("{:x}", Sha256::digest(fs::read(&target)?));
    let stage = tempfile::Builder::new()
        .prefix(".murr-update-")
        .tempdir_in(directory)?;
    let staged_exe = stage.path().join("download.exe");
    let mut file = File::create(&staged_exe)?;
    file.write_all(&update.bytes)?;
    file.sync_all()?;
    drop(file);
    let powershell = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("Папка Windows не найдена"))?,
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("install_update.ps1"),
        ])
        .env("MURR_INSTALL_TARGET", target)
        .env("MURR_INSTALL_DOWNLOAD", staged_exe)
        .env("MURR_INSTALL_PID", std::process::id().to_string())
        .env("MURR_INSTALL_OLD_HASH", current_hash)
        .env(
            "MURR_INSTALL_NEW_HASH",
            format!("{:x}", Sha256::digest(&update.bytes)),
        )
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()?;
    let _ = stage.keep();
    Ok(())
}

#[cfg(not(windows))]
fn install(_: &Update) -> std::io::Result<()> {
    Err(std::io::Error::other("Автообновление доступно в Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_newer_stable_windows_releases_only() {
        let mut release: Release = serde_json::from_value(serde_json::json!({
            "tag_name": "v0.10.0", "draft": false, "prerelease": false,
            "assets": [{"id": 42, "name": ASSET_NAME, "size": 10, "state": "uploaded"}]
        }))
        .unwrap();
        let installed = Version::parse("0.9.0").unwrap();
        assert_eq!(
            asset_for_release(&release, &installed)
                .unwrap()
                .unwrap()
                .1
                .id,
            42
        );
        release.prerelease = true;
        assert!(asset_for_release(&release, &installed).unwrap().is_none());
        release.prerelease = false;
        release.tag_name = "v0.8.0".into();
        assert!(asset_for_release(&release, &installed).unwrap().is_none());
    }

    #[test]
    fn verifies_the_size_and_digest_before_installing() {
        let data = b"MZwindows executable";
        let size = data.len() as u64;
        let digest = format!("{:x}", Sha256::digest(data));
        assert_eq!(verify_exe(data.as_slice(), size, &digest).unwrap(), data);
        assert!(verify_exe(data.as_slice(), size + 1, &digest).is_err());
        assert!(verify_exe(b"MZmodified executable".as_slice(), size, &digest).is_err());
    }
}
