use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};

use log::warn;
use thiserror::Error;

use crate::config::{AurHelperMode, EffectiveConfig};
use crate::parser::parse_update_lines;
use crate::state::{UpdateSnapshot, UpdateSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectedAurHelper {
    Paru,
    Yay,
}

impl DetectedAurHelper {
    pub fn binary(self) -> &'static str {
        match self {
            Self::Paru => "paru",
            Self::Yay => "yay",
        }
    }
}

impl fmt::Display for DetectedAurHelper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.binary())
    }
}

#[derive(Clone, Debug)]
struct ResolvedCommand {
    program: String,
    args: Vec<String>,
}

pub struct CheckOutcome {
    pub snapshot: UpdateSnapshot,
    pub helper: Option<DetectedAurHelper>,
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("failed to spawn `{program}`: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("command `{command}` exited with {status}: {stderr}")]
    NonZero {
        command: String,
        status: i32,
        stderr: String,
    },
    #[error("invalid configured command `{0}`")]
    InvalidCommand(String),
    #[error("failed filesystem operation ({context}): {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}

pub fn perform_check(config: &EffectiveConfig) -> Result<CheckOutcome, CommandError> {
    let official = run_official_check(config)?;
    let helper = detect_aur_helper(config.aur_helper, config.enable_aur);

    let aur = if config.enable_aur {
        if let Some(helper) = helper {
            run_aur_check(helper)?
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(CheckOutcome {
        snapshot: UpdateSnapshot { official, aur },
        helper,
    })
}

pub fn detect_aur_helper(mode: AurHelperMode, enable_aur: bool) -> Option<DetectedAurHelper> {
    if !enable_aur {
        return None;
    }

    let path = env::var_os("PATH");
    detect_aur_helper_with_path(mode, path.as_deref())
}

fn detect_aur_helper_with_path(
    mode: AurHelperMode,
    path: Option<&OsStr>,
) -> Option<DetectedAurHelper> {
    match mode {
        AurHelperMode::None => None,
        AurHelperMode::Paru => has_binary("paru", path).then_some(DetectedAurHelper::Paru),
        AurHelperMode::Yay => has_binary("yay", path).then_some(DetectedAurHelper::Yay),
        AurHelperMode::Auto => {
            if has_binary("paru", path) {
                Some(DetectedAurHelper::Paru)
            } else if has_binary("yay", path) {
                Some(DetectedAurHelper::Yay)
            } else {
                None
            }
        }
    }
}

fn has_binary(binary: &str, path: Option<&OsStr>) -> bool {
    let path_value = path
        .map(|p| p.to_os_string())
        .or_else(|| env::var_os("PATH"));
    let Some(path_value) = path_value else {
        return false;
    };

    for dir in env::split_paths(&path_value) {
        let candidate = dir.join(binary);
        if is_executable_file(&candidate) {
            return true;
        }
    }

    false
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    match path.metadata() {
        Ok(meta) => meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

fn run_official_check(
    config: &EffectiveConfig,
) -> Result<Vec<crate::state::PackageUpdate>, CommandError> {
    if config.official_check_cmd != "auto" {
        return run_official_check_custom(config);
    }

    let db_path = checkupdates_db_path();
    prepare_checkupdates_db(&db_path)?;
    let _guard = DbLockGuard::new(db_path.join("db.lck"));

    sync_checkupdates_db(&db_path)?;
    let out = query_official_updates(&db_path)?;
    let filtered = filter_pacman_qu_output(&out.stdout);

    Ok(parse_update_lines(&filtered, UpdateSource::Official))
}

fn run_official_check_custom(
    config: &EffectiveConfig,
) -> Result<Vec<crate::state::PackageUpdate>, CommandError> {
    let mut cmd = parse_command_string(&config.official_check_cmd)?;
    cmd.args.push("--nocolor".to_string());
    let out = run_capture(&cmd, &[0, 2])?;
    Ok(parse_update_lines(&out.stdout, UpdateSource::Official))
}

fn run_aur_check(
    helper: DetectedAurHelper,
) -> Result<Vec<crate::state::PackageUpdate>, CommandError> {
    let cmd = ResolvedCommand {
        program: helper.binary().to_string(),
        args: vec!["-Qua".to_string()],
    };

    let out = run_capture(&cmd, &[0, 1])?;
    Ok(parse_update_lines(&out.stdout, UpdateSource::Aur))
}

const DEFAULT_DBPATH: &str = "/var/lib/pacman";
const DEFAULT_TMPDIR: &str = "/tmp";
const DEFAULT_UID: &str = "0";

fn checkupdates_db_path() -> PathBuf {
    checkupdates_db_path_from_inputs(
        env::var("CHECKUPDATES_DB").ok().as_deref(),
        env::var("TMPDIR").ok().as_deref(),
        env::var("UID").ok().as_deref(),
    )
}

fn checkupdates_db_path_from_inputs(
    checkupdates_db: Option<&str>,
    tmpdir: Option<&str>,
    uid: Option<&str>,
) -> PathBuf {
    if let Some(raw) = checkupdates_db {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    let tmpdir = tmpdir
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(DEFAULT_TMPDIR);
    let uid = uid.filter(|v| !v.trim().is_empty()).unwrap_or(DEFAULT_UID);
    Path::new(tmpdir).join(format!("checkup-db-{uid}"))
}

fn prepare_checkupdates_db(db_path: &Path) -> Result<(), CommandError> {
    fs::create_dir_all(db_path).map_err(|source| CommandError::Io {
        context: format!("create temp pacman db at {}", db_path.display()),
        source,
    })?;

    let real_db_path = resolve_pacman_db_path();
    let src_local = real_db_path.join("local");
    let dst_local = db_path.join("local");

    if !dst_local.exists() {
        symlink(&src_local, &dst_local).map_err(|source| CommandError::Io {
            context: format!(
                "symlink local db from {} to {}",
                src_local.display(),
                dst_local.display()
            ),
            source,
        })?;
    }

    Ok(())
}

fn resolve_pacman_db_path() -> PathBuf {
    let cmd = ResolvedCommand {
        program: "pacman-conf".to_string(),
        args: vec!["DBPath".to_string()],
    };

    match run_capture(&cmd, &[0]) {
        Ok(output) => output
            .stdout
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DBPATH)),
        Err(err) => {
            warn!("failed to read DBPath via pacman-conf ({err}); using {DEFAULT_DBPATH}");
            PathBuf::from(DEFAULT_DBPATH)
        }
    }
}

fn sync_checkupdates_db(db_path: &Path) -> Result<(), CommandError> {
    let cmd = ResolvedCommand {
        program: "fakeroot".to_string(),
        args: vec![
            "--".to_string(),
            "pacman".to_string(),
            "-Sy".to_string(),
            "--disable-sandbox-filesystem".to_string(),
            "--dbpath".to_string(),
            db_path.display().to_string(),
            "--logfile".to_string(),
            "/dev/null".to_string(),
        ],
    };

    run_capture(&cmd, &[0]).map(|_| ())
}

fn query_official_updates(db_path: &Path) -> Result<CommandOutput, CommandError> {
    let cmd = ResolvedCommand {
        program: "pacman".to_string(),
        args: vec![
            "-Qu".to_string(),
            "--dbpath".to_string(),
            db_path.display().to_string(),
            "--color".to_string(),
            "never".to_string(),
        ],
    };

    run_capture(&cmd, &[0, 1])
}

fn filter_pacman_qu_output(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !(trimmed.contains('[') && trimmed.contains(']'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct DbLockGuard {
    lock_file: PathBuf,
}

impl DbLockGuard {
    fn new(lock_file: PathBuf) -> Self {
        Self { lock_file }
    }
}

impl Drop for DbLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_file);
    }
}

fn parse_command_string(raw: &str) -> Result<ResolvedCommand, CommandError> {
    let mut parts =
        shell_words::split(raw).map_err(|_| CommandError::InvalidCommand(raw.into()))?;
    if parts.is_empty() {
        return Err(CommandError::InvalidCommand(raw.into()));
    }

    let program = parts.remove(0);
    Ok(ResolvedCommand {
        program,
        args: parts,
    })
}

#[derive(Debug)]
struct CommandOutput {
    stdout: String,
    _stderr: String,
}

fn run_capture(
    cmd: &ResolvedCommand,
    allowed_codes: &[i32],
) -> Result<CommandOutput, CommandError> {
    let output = Command::new(&cmd.program)
        .args(&cmd.args)
        .output()
        .map_err(|source| CommandError::Spawn {
            program: cmd.program.clone(),
            source,
        })?;

    let status = output.status;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if is_allowed(status, allowed_codes) {
        return Ok(CommandOutput {
            stdout,
            _stderr: stderr,
        });
    }

    Err(CommandError::NonZero {
        command: shell_join(&cmd.program, &cmd.args),
        status: status.code().unwrap_or(-1),
        stderr: if stderr.is_empty() {
            "<no stderr>".to_string()
        } else {
            stderr
        },
    })
}

fn is_allowed(status: ExitStatus, allowed_codes: &[i32]) -> bool {
    status
        .code()
        .map(|code| allowed_codes.contains(&code))
        .unwrap_or(false)
}

fn shell_join(program: &str, args: &[String]) -> String {
    let mut all = Vec::with_capacity(args.len() + 1);
    all.push(shell_words::quote(program).to_string());
    all.extend(args.iter().map(|arg| shell_words::quote(arg).to_string()));
    all.join(" ")
}

pub fn build_details_shell_command(
    config: &EffectiveConfig,
    helper: Option<DetectedAurHelper>,
) -> Result<String, CommandError> {
    let mut pieces: Vec<String> = Vec::new();

    if config.official_check_cmd == "auto" {
        pieces.push("pacman -Qu --color never".to_string());
    } else {
        let mut official = parse_command_string(&config.official_check_cmd)?;
        official.args.push("--nocolor".to_string());
        pieces.push(shell_join(&official.program, &official.args));
    }

    if config.enable_aur {
        if let Some(h) = helper {
            pieces.push("echo".to_string());
            pieces.push(format!("{} -Qua", h.binary()));
        } else {
            pieces.push("echo".to_string());
            pieces.push("echo 'AUR helper not found (expected paru or yay)'".to_string());
        }
    }

    pieces.push("echo".to_string());
    pieces.push("read -n 1 -s -r -p 'Press any key to close...'".to_string());
    Ok(pieces.join("; "))
}

pub fn build_upgrade_shell_command(
    config: &EffectiveConfig,
    helper: Option<DetectedAurHelper>,
) -> String {
    if config.upgrade_cmd != "auto" {
        return config.upgrade_cmd.clone();
    }

    match helper {
        Some(h) => format!("{} -Syu", h.binary()),
        None => "sudo pacman -Syu".to_string(),
    }
}

pub fn build_upgrade_official_shell_command() -> String {
    "sudo pacman -Syu".to_string()
}

pub fn build_upgrade_aur_shell_command(helper: Option<DetectedAurHelper>) -> Option<String> {
    helper.map(|h| format!("{} -Sua", h.binary()))
}

pub fn launch_in_terminal(
    config: &EffectiveConfig,
    shell_command: &str,
) -> Result<(), CommandError> {
    launch_in_terminal_process(config, shell_command)?;
    Ok(())
}

pub fn launch_in_terminal_process(
    config: &EffectiveConfig,
    shell_command: &str,
) -> Result<Child, CommandError> {
    let terminal = resolve_terminal(&config.terminal).ok_or_else(|| {
        CommandError::InvalidCommand("no supported terminal found (set terminal in config)".into())
    })?;

    let mut cmd = Command::new(&terminal.program);
    cmd.args(&terminal.args);

    if terminal.exec_delimiter == "--" {
        cmd.arg("--").arg("bash").arg("-lc").arg(shell_command);
    } else {
        cmd.arg(&terminal.exec_delimiter)
            .arg("bash")
            .arg("-lc")
            .arg(shell_command);
    }

    cmd.spawn().map_err(|source| CommandError::Spawn {
        program: terminal.program,
        source,
    })
}

#[derive(Debug)]
struct TerminalSpec {
    program: String,
    args: Vec<String>,
    exec_delimiter: String,
}

fn resolve_terminal(configured: &str) -> Option<TerminalSpec> {
    if configured != "auto" {
        return parse_terminal_spec(configured).ok();
    }

    if let Ok(from_env) = env::var("TERMINAL") {
        if let Ok(parsed) = parse_terminal_spec(&from_env) {
            return Some(parsed);
        }
        warn!("failed to parse TERMINAL={from_env}, falling back to defaults");
    }

    let fallback = [
        "kitty",
        "alacritty",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ];

    for candidate in fallback {
        if has_binary(candidate, env::var_os("PATH").as_deref()) {
            return Some(TerminalSpec {
                program: candidate.to_string(),
                args: Vec::new(),
                exec_delimiter: terminal_exec_delimiter(candidate).to_string(),
            });
        }
    }

    None
}

fn parse_terminal_spec(raw: &str) -> Result<TerminalSpec, CommandError> {
    let mut parts =
        shell_words::split(raw).map_err(|_| CommandError::InvalidCommand(raw.into()))?;
    if parts.is_empty() {
        return Err(CommandError::InvalidCommand(raw.into()));
    }

    let program = parts.remove(0);
    let base = Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(program.as_str())
        .to_string();

    Ok(TerminalSpec {
        program,
        args: parts,
        exec_delimiter: terminal_exec_delimiter(&base).to_string(),
    })
}

fn terminal_exec_delimiter(program_name: &str) -> &'static str {
    match program_name {
        "gnome-terminal" => "--",
        _ => "-e",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn create_mock_binary(dir: &Path, name: &str) {
        let path = dir.join(name);
        fs::write(&path, "#!/usr/bin/env bash\nexit 0\n").expect("write mock binary");
        let mut perms = fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod");
    }

    #[test]
    fn auto_helper_prefers_paru_over_yay() {
        let temp = tempfile::tempdir().expect("tempdir");
        create_mock_binary(temp.path(), "paru");
        create_mock_binary(temp.path(), "yay");

        let helper =
            detect_aur_helper_with_path(AurHelperMode::Auto, Some(temp.path().as_os_str()));

        assert_eq!(helper, Some(DetectedAurHelper::Paru));
    }

    #[test]
    fn explicit_yay_requires_binary_to_exist() {
        let temp = tempfile::tempdir().expect("tempdir");
        let helper = detect_aur_helper_with_path(AurHelperMode::Yay, Some(temp.path().as_os_str()));
        assert_eq!(helper, None);

        create_mock_binary(temp.path(), "yay");
        let helper = detect_aur_helper_with_path(AurHelperMode::Yay, Some(temp.path().as_os_str()));
        assert_eq!(helper, Some(DetectedAurHelper::Yay));
    }

    #[test]
    fn checkupdates_db_path_prefers_env_override() {
        let path = checkupdates_db_path_from_inputs(Some("/custom/check-db"), None, None);
        assert_eq!(path, PathBuf::from("/custom/check-db"));
    }

    #[test]
    fn checkupdates_db_path_uses_tmpdir_and_uid_defaults() {
        let path = checkupdates_db_path_from_inputs(None, Some("/tmpx"), Some("1234"));
        assert_eq!(path, PathBuf::from("/tmpx/checkup-db-1234"));
    }

    #[test]
    fn filter_pacman_qu_output_drops_bracket_lines() {
        let input = "pacman 1.0-1 -> 1.0-2\nwarning: [ignored package]\nopenssl 3.1-1 -> 3.1-2\n";
        let out = filter_pacman_qu_output(input);
        assert_eq!(out, "pacman 1.0-1 -> 1.0-2\nopenssl 3.1-1 -> 3.1-2");
    }
}
