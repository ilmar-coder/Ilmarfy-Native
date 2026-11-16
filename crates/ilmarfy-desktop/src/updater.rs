//! Atualização pelo próprio app: procura a última Release `desktop-v*` no GitHub,
//! baixa o pacote do sistema (zip do .app no Mac, RPM no Fedora) e instala.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const RELEASES: &str = "https://api.github.com/repos/ilmar-coder/Ilmarfy-Native/releases?per_page=20";
/// Nome do pacote do Mac na Release (gerado pelo workflow do GitHub)
pub const MAC_ASSET: &str = "Ilmarfy-macos-arm64.zip";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(600)))
        .user_agent(concat!("ilmarfy-desktop/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn asset_matches(name: &str) -> bool {
    if cfg!(target_os = "macos") {
        name == MAC_ASSET
    } else {
        name.ends_with(&format!("{}.rpm", std::env::consts::ARCH))
    }
}

/// A Release mais nova que a versão atual com pacote para este sistema, se houver.
pub fn check() -> Result<Option<Release>, String> {
    let list: Vec<GhRelease> = agent()
        .get(RELEASES)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Sem acesso ao GitHub: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    let current = env!("CARGO_PKG_VERSION");
    let best = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = r.tag_name.strip_prefix("desktop-v")?.to_string();
            let asset = r.assets.into_iter().find(|a| asset_matches(&a.name))?;
            Some(Release {
                version,
                notes: r.body.unwrap_or_default().trim().to_string(),
                url: asset.browser_download_url,
                size: asset.size,
            })
        })
        .filter(|r| is_newer(&r.version, current))
        .max_by_key(|r| parse_version(&r.version));
    Ok(best)
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    let mut resp = agent().get(url).call().map_err(|e| format!("Falha ao baixar: {e}"))?;
    let tmp = dest.with_extension("part");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    std::io::copy(&mut resp.body_mut().as_reader(), &mut file).map_err(|e| format!("Falha ao baixar: {e}"))?;
    drop(file);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Baixa e instala. Devolve o comando que abre a nova versão (rodar logo antes de sair).
pub fn install(rel: &Release, work_dir: &Path) -> Result<Command, String> {
    let dir = work_dir.join("update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        install_mac(rel, &dir)
    } else {
        install_linux(rel, &dir)
    }
}

/// O .app em que este executável está (…/Ilmarfy.app/Contents/MacOS/ilmarfy)
fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let bundle = exe.parent()?.parent()?.parent()?.to_path_buf();
    (bundle.extension()? == "app").then_some(bundle)
}

fn install_mac(rel: &Release, dir: &Path) -> Result<Command, String> {
    let bundle = current_bundle().ok_or("O app não está num pacote .app; atualize com o bundle-macos.sh")?;
    let parent = bundle.parent().ok_or("Pasta do app inválida")?;
    let zip = dir.join(MAC_ASSET);
    download(&rel.url, &zip)?;
    let unpacked = dir.join("unpacked");
    run(Command::new("ditto").args(["-x", "-k"]).arg(&zip).arg(&unpacked))?;
    let new_app = unpacked.join("Ilmarfy.app");
    if !new_app.join("Contents/MacOS/ilmarfy").is_file() {
        return Err("Pacote baixado não tem o Ilmarfy.app".into());
    }
    // Copia ao lado do atual e troca com renomeações (rápido e reversível)
    let staged = parent.join(".Ilmarfy-novo.app");
    let old = parent.join(".Ilmarfy-antigo.app");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&old);
    run(Command::new("ditto").arg(&new_app).arg(&staged)).map_err(|e| format!("Sem permissão em {}: {e}", parent.display()))?;
    std::fs::rename(&bundle, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&staged, &bundle) {
        let _ = std::fs::rename(&old, &bundle);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&old);
    let mut relaunch = Command::new("sh");
    relaunch.arg("-c").arg(format!("sleep 1; open -n '{}'", bundle.display()));
    Ok(relaunch)
}

fn install_linux(rel: &Release, dir: &Path) -> Result<Command, String> {
    let rpm = dir.join(format!("ilmarfy-{}.rpm", rel.version));
    download(&rel.url, &rpm)?;
    // pkexec mostra a janela de senha do próprio sistema
    run(Command::new("pkexec").args(["dnf", "install", "-y"]).arg(&rpm)).map_err(|e| {
        if e.is_empty() || e.contains("dismissed") || e.contains("Not authorized") {
            "Instalação cancelada".to_string()
        } else {
            format!("dnf falhou: {e}")
        }
    })?;
    let mut relaunch = Command::new("sh");
    relaunch.arg("-c").arg("sleep 1; setsid /usr/bin/ilmarfy >/dev/null 2>&1 &");
    Ok(relaunch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versoes() {
        assert!(is_newer("0.3.0", "0.2.1"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.0", "0.2.1"));
        assert!(!is_newer("lixo", "0.2.1"));
    }

    #[test]
    #[ignore = "acessa o GitHub"]
    fn consulta_releases() {
        println!("{:?}", check());
    }
}
