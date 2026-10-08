//! The web app's "Open in obj2cad" button: an `obj2cad://convert?…` link, which the
//! operating system hands to this tool (`obj2cad open-url <link>`, in a terminal window)
//! once `obj2cad setup` has registered it.
//!
//! A browser can't tell anything where the file it was given lives, so the link carries
//! the file's name and size and the settings chosen in the app; the file is looked for in
//! the usual places (Downloads, Desktop, Documents) and nothing is converted until the
//! person says which file it is. Links can come from any page, so every value is checked
//! against what the app sends, and the output always goes next to the input.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SCHEME: &str = "obj2cad";

/// A link from the web app.
#[derive(Debug, PartialEq)]
pub struct Link {
    /// The file (or folder) the app was given, by name.
    pub name: String,
    /// Its size in bytes, when it is one file.
    pub size: Option<u64>,
    /// `convert` options for the settings chosen in the app.
    pub options: Vec<String>,
}

/// Read a link (`obj2cad://convert?v=1&name=…&size=…&format=…`). Unknown parameters are
/// ignored (a newer app); a value the app wouldn't send is an error.
pub fn parse_link(url: &str) -> Result<Link, String> {
    let rest = url
        .strip_prefix(&format!("{SCHEME}://"))
        .or_else(|| url.strip_prefix(&format!("{SCHEME}:")))
        .ok_or("not an obj2cad link")?;
    let (action, query) = rest.split_once('?').unwrap_or((rest, ""));
    if action.trim_end_matches('/') != "convert" {
        return Err(format!("unknown obj2cad link `{action}`"));
    }
    let one_of = |key: &str, v: &str, allowed: &[&str]| -> Result<(), String> {
        if allowed.contains(&v) {
            Ok(())
        } else {
            Err(format!("the link's {key} `{v}` isn't one obj2cad knows"))
        }
    };
    let (mut name, mut size, mut options) = (None, None, Vec::new());
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = percent_decode(v)?;
        match k {
            "v" if v != "1" => {
                return Err("this link is from a newer obj2cad; update this one".into())
            }
            "name" => name = Some(v),
            "size" => {
                size = Some(
                    v.parse::<u64>()
                        .map_err(|_| "the link's size isn't a number")?,
                )
            }
            "format" => {
                one_of(k, &v, &["dxf", "dxf-binary", "dwg"])?;
                options.extend(["--format".into(), v]);
            }
            "units" | "default-units" => {
                let units: &[&str] = if k == "units" {
                    &["auto", "unitless", "mm", "cm", "m", "in", "ft"]
                } else {
                    &["mm", "cm", "m", "in", "ft"]
                };
                one_of(k, &v, units)?;
                options.extend([format!("--{k}"), v]);
            }
            "up" => {
                one_of(k, &v, &["auto", "as-is", "y-to-z"])?;
                options.extend(["--up".into(), v]);
            }
            "layers" => {
                one_of(k, &v, &["objects", "groups", "materials", "single"])?;
                options.extend(["--layers".into(), v]);
            }
            "curves" | "keep-loose-points" => {
                one_of(k, &v, &["0", "1"])?;
                if v == "1" {
                    options.push(format!("--{k}"));
                }
            }
            "exclude-layer" => {
                if v.is_empty() || v.chars().any(char::is_control) {
                    return Err("the link names a layer obj2cad can't use".into());
                }
                options.extend(["--exclude-layer".into(), v]);
            }
            _ => {}
        }
    }
    let name = name.ok_or("the link doesn't name a file")?;
    let plain = !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control);
    if !plain {
        return Err("the link's file name isn't a plain file name".into());
    }
    Ok(Link {
        name,
        size,
        options,
    })
}

fn percent_decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = b
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .ok_or("the link is malformed")?;
                out.push(hex);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "the link is malformed".into())
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Where a browser's files usually are.
fn search_dirs() -> Vec<PathBuf> {
    let Some(home) = home() else {
        return Vec::new();
    };
    ["Downloads", "Desktop", "Documents"]
        .iter()
        .map(|d| home.join(d))
        .filter(|d| d.is_dir())
        .collect()
}

/// Files (or folders) called `name` (ignoring case) in `dirs` and their folders, of
/// `size` bytes when it is given.
fn find(dirs: &[PathBuf], name: &str, size: Option<u64>) -> Vec<PathBuf> {
    let matches = |p: &Path| {
        let Ok(meta) = std::fs::metadata(p) else {
            return false;
        };
        meta.is_dir() || size.is_none_or(|s| meta.len() == s)
    };
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) && matches(&path) {
                found.push(path);
            } else if e.file_type().is_ok_and(|t| t.is_dir()) {
                let inner = path.join(name);
                if inner.exists() && matches(&inner) {
                    found.push(inner);
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// A path as dropped or typed into a terminal: quoted on Windows, backslash-escaped on
/// macOS, sometimes a `file://` URL on Linux.
pub fn dropped_path(line: &str) -> PathBuf {
    let t = line.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))
        .unwrap_or(t);
    if let Some(url) = t.strip_prefix("file://") {
        if let Ok(p) = percent_decode(&url.replace('+', "%2B")) {
            return PathBuf::from(p);
        }
    }
    if cfg!(windows) {
        return PathBuf::from(t);
    }
    let mut out = String::with_capacity(t.len());
    let mut chars = t.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    PathBuf::from(out)
}

fn ask(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    let n = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("no answer".into());
    }
    Ok(line.trim().to_owned())
}

fn bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.2} GB", n as f64 / f64::from(1 << 30)),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / f64::from(1 << 20)),
        n => format!("{n} bytes"),
    }
}

/// Which file the link means: found and confirmed, chosen among several, or dropped in.
fn choose(link: &Link) -> Result<PathBuf, String> {
    let dirs = search_dirs();
    let found = find(&dirs, &link.name, link.size);
    let drop_in = |why: &str| -> Result<PathBuf, String> {
        println!("{why}");
        let line = ask("Drag it into this window (or type where it is), then press Enter: ")?;
        let path = dropped_path(&line);
        if path.exists() {
            Ok(path)
        } else {
            Err(format!("{}: no such file", path.display()))
        }
    };
    let describe = |p: &Path| match std::fs::metadata(p) {
        Ok(m) if m.is_file() => format!("{}  ({})", p.display(), bytes(m.len())),
        _ => p.display().to_string(),
    };
    match found.as_slice() {
        [] => drop_in(&format!(
            "{} isn't in your Downloads, Desktop or Documents.",
            link.name
        )),
        [one] => {
            println!("Found {}", describe(one));
            match ask("Convert it? [Y/n] ")?.to_ascii_lowercase().as_str() {
                "" | "y" | "yes" => Ok(one.clone()),
                _ => drop_in("Which file, then?"),
            }
        }
        several => {
            println!("There are {} files called {}:", several.len(), link.name);
            for (i, p) in several.iter().enumerate() {
                println!("  {}. {}", i + 1, describe(p));
            }
            let answer = ask(&format!(
                "Which one? [1-{}, or drag another file here] ",
                several.len()
            ))?;
            match answer.parse::<usize>() {
                Ok(i) if (1..=several.len()).contains(&i) => Ok(several[i - 1].clone()),
                _ => {
                    let path = dropped_path(&answer);
                    if path.exists() {
                        Ok(path)
                    } else {
                        Err(format!("{}: no such file", path.display()))
                    }
                }
            }
        }
    }
}

/// `obj2cad open-url <link>`: convert the file the web app was given, with its settings.
pub fn open_url(url: &str, convert: fn(Vec<String>) -> Result<(), String>) -> Result<(), String> {
    println!(
        "obj2cad {} — a file from the web app\n",
        obj2cad_core::VERSION
    );
    let link = parse_link(url)?;
    let input = choose(&link)?;
    println!(
        "\nConverting {}… (a large scan takes a few minutes)",
        input.display()
    );
    let mut args = vec![input.to_string_lossy().into_owned()];
    args.extend(link.options);
    convert(args)?;
    println!("\nDone: the drawing is next to the file it came from.");
    Ok(())
}

/// Keep a window opened for this tool open until its output has been read.
pub fn pause() {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        let _ = ask("\nPress Enter to close this window.");
    }
}

// ---------------------------------------------------------------- setup

/// Where `setup` keeps its copy of this tool (so the link keeps working when the
/// download is moved or deleted).
fn install_path() -> Result<PathBuf, String> {
    let dir = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("obj2cad"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/obj2cad"))
    } else {
        home().map(|h| h.join(".local/share/obj2cad"))
    }
    .ok_or("can't find your home folder")?;
    Ok(dir.join(if cfg!(windows) {
        "obj2cad.exe"
    } else {
        "obj2cad"
    }))
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} {} failed", args.join(" ")))
    }
}

/// `obj2cad setup`: copy this tool where it can stay, and register `obj2cad://` links for
/// this user (no administrator rights). `remove` undoes it.
pub fn setup(remove: bool) -> Result<(), String> {
    let installed = install_path()?;
    if remove {
        unregister(&installed)?;
        if std::fs::remove_file(&installed).is_err() && installed.exists() {
            println!(
                "(close any obj2cad windows, then delete {})",
                installed.display()
            );
        }
        println!("obj2cad links are no longer opened here.");
        return Ok(());
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let same = me.canonicalize().ok() == installed.canonicalize().ok();
    if !same {
        let dir = installed.parent().expect("a file path");
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::copy(&me, &installed).map_err(|e| {
            format!(
                "{}: {e} (close any obj2cad windows and try again)",
                installed.display()
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755));
        }
    }
    register(&installed)?;
    println!(
        "obj2cad {} is set up: \"Open in obj2cad\" in the web app opens files here.\n  installed at {}",
        obj2cad_core::VERSION,
        installed.display()
    );
    Ok(())
}

#[cfg(windows)]
fn register(exe: &Path) -> Result<(), String> {
    let key = format!(r"HKCU\Software\Classes\{SCHEME}");
    let command = format!("\"{}\" open-url \"%1\"", exe.display());
    run("reg", &["add", &key, "/ve", "/d", "URL:obj2cad", "/f"])?;
    run("reg", &["add", &key, "/v", "URL Protocol", "/d", "", "/f"])?;
    run(
        "reg",
        &[
            "add",
            &format!(r"{key}\shell\open\command"),
            "/ve",
            "/d",
            &command,
            "/f",
        ],
    )
}

#[cfg(windows)]
fn unregister(_exe: &Path) -> Result<(), String> {
    run(
        "reg",
        &["delete", &format!(r"HKCU\Software\Classes\{SCHEME}"), "/f"],
    )
    .or(Ok(()))
}

#[cfg(target_os = "macos")]
fn app_path() -> Result<PathBuf, String> {
    Ok(home()
        .ok_or("can't find your home folder")?
        .join("Applications/obj2cad.app"))
}

#[cfg(target_os = "macos")]
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// macOS only gives links to apps (through an Apple event, not the command line): a small
/// AppleScript applet receives them and runs this tool in Terminal.
#[cfg(target_os = "macos")]
fn register(exe: &Path) -> Result<(), String> {
    let _ = run(
        "xattr",
        &["-d", "com.apple.quarantine", &exe.to_string_lossy()],
    );
    let app = app_path()?;
    let script = format!(
        "on open location theURL\n\
         \ttell application \"Terminal\"\n\
         \t\tactivate\n\
         \t\tdo script (quoted form of \"{}\") & \" open-url \" & (quoted form of theURL)\n\
         \tend tell\n\
         end open location\n",
        exe.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    let src = std::env::temp_dir().join("obj2cad-open.applescript");
    std::fs::write(&src, script).map_err(|e| e.to_string())?;
    if let Some(dir) = app.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&app);
    run(
        "osacompile",
        &["-o", &app.to_string_lossy(), &src.to_string_lossy()],
    )?;
    let plist = app.join("Contents/Info.plist");
    let plist = plist.to_string_lossy();
    for cmd in [
        "Delete :CFBundleIdentifier",
        "Add :CFBundleIdentifier string com.cbassuarez.obj2cad.open",
        "Add :LSUIElement bool true",
        "Add :CFBundleURLTypes array",
        "Add :CFBundleURLTypes:0 dict",
        "Add :CFBundleURLTypes:0:CFBundleURLName string obj2cad",
        "Add :CFBundleURLTypes:0:CFBundleURLSchemes array",
        "Add :CFBundleURLTypes:0:CFBundleURLSchemes:0 string obj2cad",
    ] {
        // Deleting an identifier that isn't there is fine.
        let r = run("/usr/libexec/PlistBuddy", &["-c", cmd, &plist]);
        if !cmd.starts_with("Delete") {
            r?;
        }
    }
    run(LSREGISTER, &["-f", &app.to_string_lossy()])
}

#[cfg(target_os = "macos")]
fn unregister(_exe: &Path) -> Result<(), String> {
    let app = app_path()?;
    let _ = run(LSREGISTER, &["-u", &app.to_string_lossy()]);
    let _ = std::fs::remove_dir_all(&app);
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_file() -> Result<PathBuf, String> {
    Ok(home()
        .ok_or("can't find your home folder")?
        .join(".local/share/applications/obj2cad-url.desktop"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn register(exe: &Path) -> Result<(), String> {
    let file = desktop_file()?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=obj2cad\nExec=\"{}\" open-url %u\nTerminal=true\nNoDisplay=true\nMimeType=x-scheme-handler/{SCHEME};\n",
        exe.display()
    );
    std::fs::write(&file, entry).map_err(|e| format!("{}: {e}", file.display()))?;
    run(
        "xdg-mime",
        &[
            "default",
            "obj2cad-url.desktop",
            &format!("x-scheme-handler/{SCHEME}"),
        ],
    )?;
    if let Some(dir) = file.parent() {
        let _ = run("update-desktop-database", &[&dir.to_string_lossy()]);
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn unregister(_exe: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(desktop_file()?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_become_convert_options() {
        let l = parse_link("obj2cad://convert?v=1&name=scan%20(2).zip&size=680571702&format=dwg&units=mm&up=y-to-z&layers=materials&curves=1&keep-loose-points=0&exclude-layer=a%26b&future=x").unwrap();
        assert_eq!(l.name, "scan (2).zip");
        assert_eq!(l.size, Some(680_571_702));
        assert_eq!(
            l.options,
            [
                "--format",
                "dwg",
                "--units",
                "mm",
                "--up",
                "y-to-z",
                "--layers",
                "materials",
                "--curves",
                "--exclude-layer",
                "a&b"
            ]
        );
    }

    #[test]
    fn links_are_checked() {
        for bad in [
            "https://example.com/convert?name=a.obj",
            "obj2cad://delete?name=a.obj",
            "obj2cad://convert?format=dwg",
            "obj2cad://convert?name=..%2F..%2Fsecret",
            "obj2cad://convert?name=C%3A%5Cx.obj",
            "obj2cad://convert?name=a.obj&format=exe",
            "obj2cad://convert?name=a.obj&units=parsecs",
            "obj2cad://convert?name=a.obj&size=-1",
            "obj2cad://convert?name=a.obj&v=2",
            "obj2cad://convert?name=a%ZZ.obj",
        ] {
            assert!(parse_link(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn dropped_paths_lose_their_quoting() {
        assert_eq!(
            dropped_path("  \"C:\\My Files\\a.zip\"  "),
            PathBuf::from(if cfg!(windows) {
                "C:\\My Files\\a.zip"
            } else {
                "C:My Filesa.zip"
            })
        );
        assert_eq!(
            dropped_path("'/tmp/a b.zip'\n"),
            PathBuf::from("/tmp/a b.zip")
        );
        assert_eq!(
            dropped_path("file:///tmp/a%20b.zip"),
            PathBuf::from("/tmp/a b.zip")
        );
        if !cfg!(windows) {
            assert_eq!(
                dropped_path("/Users/x/My\\ Scan.zip"),
                PathBuf::from("/Users/x/My Scan.zip")
            );
        }
    }

    #[test]
    fn finds_the_file_by_name_and_size_in_the_usual_places() {
        let root = std::env::temp_dir().join(format!("obj2cad-find-{}", std::process::id()));
        let (dl, desk) = (root.join("Downloads"), root.join("Desktop"));
        std::fs::create_dir_all(dl.join("sub")).unwrap();
        std::fs::create_dir_all(&desk).unwrap();
        std::fs::write(dl.join("Scan.zip"), b"12345").unwrap();
        std::fs::write(dl.join("sub").join("scan.zip"), b"123").unwrap();
        std::fs::write(desk.join("scan.zip"), b"12345").unwrap();
        let dirs = [dl.clone(), desk.clone()];
        let mut all = find(&dirs, "scan.zip", None);
        all.sort();
        assert_eq!(all.len(), 3);
        assert_eq!(
            find(&dirs, "scan.zip", Some(3)),
            [dl.join("sub").join("scan.zip")]
        );
        assert_eq!(find(&dirs, "scan.zip", Some(5)).len(), 2);
        assert!(find(&dirs, "other.zip", None).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
