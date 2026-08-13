//! `mimofan install-deps` — 探测缺失的可选工具依赖并打印/执行安装命令。
//!
//! 与 `doctor` 只打印提示不同，`install-deps` 把 doctor 里散落的跨平台安装
//! 建议结构化，默认 dry-run（只打印命令），传 `--yes` 才真正执行。执行侧
//! 走与工具执行相同的 `std::process::Command` 通道，不引入额外提权逻辑——
//! 需要 root 的 Linux 包管理器命令由调用方自行加 `sudo`（见 [`PackageManager`]）。

use anyhow::Result;
use std::process::Command;

use crate::dependencies::{
    probe_executable, resolve_node, resolve_pandoc, resolve_pdftotext, resolve_python_interpreter,
    resolve_tesseract,
};

/// 一个可被 mimofan 工具链使用的外部依赖。
#[derive(Debug, Clone, Copy)]
enum Dep {
    Python,
    Node,
    Pandoc,
    Tesseract,
    Poppler,
}

impl Dep {
    fn label(self) -> &'static str {
        match self {
            Dep::Python => "Python",
            Dep::Node => "Node.js",
            Dep::Pandoc => "pandoc",
            Dep::Tesseract => "tesseract",
            Dep::Poppler => "poppler (pdftotext)",
        }
    }

    /// 该依赖是否已在 PATH 上。
    fn is_present(self) -> bool {
        match self {
            Dep::Python => resolve_python_interpreter().is_some(),
            Dep::Node => resolve_node().is_some(),
            Dep::Pandoc => resolve_pandoc().is_some(),
            Dep::Tesseract => resolve_tesseract().is_some(),
            Dep::Poppler => resolve_pdftotext().is_some(),
        }
    }

    /// 缺失时是否影响核心能力（false = 仅降级某个可选工具）。
    fn required(self) -> bool {
        matches!(self, Dep::Python | Dep::Node)
    }

    /// 返回该依赖在当前平台包管理器下的安装命令（不含 sudo 前缀）。
    fn install_command(self, pm: PackageManager) -> Option<String> {
        let os = std::env::consts::OS;
        match (self, pm) {
            (_, PackageManager::Brew) => macos_brew(self),
            (_, PackageManager::Apt) => linux_apt(self),
            (_, PackageManager::Dnf) => linux_dnf(self),
            (_, PackageManager::Apk) => linux_apk(self),
            (_, PackageManager::Winget) => windows_winget(self),
            (_, PackageManager::Unknown) => platform_fallback(self, os),
        }
    }
}

fn macos_brew(dep: Dep) -> Option<String> {
    Some(
        match dep {
            Dep::Python => "brew install python@3.12",
            Dep::Node => "brew install node",
            Dep::Pandoc => "brew install pandoc",
            Dep::Tesseract => "brew install tesseract",
            Dep::Poppler => "brew install poppler",
        }
        .to_string(),
    )
}

fn linux_apt(dep: Dep) -> Option<String> {
    Some(
        match dep {
            Dep::Python => "sudo apt install -y python3",
            Dep::Node => "sudo apt install -y nodejs",
            Dep::Pandoc => "sudo apt install -y pandoc",
            Dep::Tesseract => "sudo apt install -y tesseract-ocr",
            Dep::Poppler => "sudo apt install -y poppler-utils",
        }
        .to_string(),
    )
}

fn linux_dnf(dep: Dep) -> Option<String> {
    Some(
        match dep {
            Dep::Python => "sudo dnf install -y python3",
            Dep::Node => "sudo dnf install -y nodejs",
            Dep::Pandoc => "sudo dnf install -y pandoc",
            Dep::Tesseract => "sudo dnf install -y tesseract",
            Dep::Poppler => "sudo dnf install -y poppler-utils",
        }
        .to_string(),
    )
}

fn linux_apk(dep: Dep) -> Option<String> {
    Some(
        match dep {
            Dep::Python => "sudo apk add python3",
            Dep::Node => "sudo apk add nodejs",
            Dep::Pandoc => "sudo apk add pandoc",
            Dep::Tesseract => "sudo apk add tesseract-ocr",
            Dep::Poppler => "sudo apk add poppler-utils",
        }
        .to_string(),
    )
}

fn windows_winget(dep: Dep) -> Option<String> {
    Some(
        match dep {
            Dep::Python => "winget install Python.Python.3",
            Dep::Node => "winget install OpenJS.NodeJS",
            Dep::Pandoc => "winget install JohnMacFarlane.Pandoc",
            Dep::Tesseract => "winget install UB-Mannheim.TesseractOCR",
            // Windows 上 pdftotext 无 winget 自动化路径
            Dep::Poppler => return None,
        }
        .to_string(),
    )
}

fn platform_fallback(dep: Dep, os: &str) -> Option<String> {
    match os {
        "macos" => macos_brew(dep),
        "linux" => linux_apt(dep),
        "windows" => windows_winget(dep),
        _ => match dep {
            Dep::Python => Some("install Python 3 from python.org".to_string()),
            Dep::Node => Some("install Node.js from nodejs.org".to_string()),
            Dep::Pandoc => Some("install pandoc from pandoc.org".to_string()),
            Dep::Tesseract => Some("install tesseract from tesseract-ocr.github.io".to_string()),
            Dep::Poppler => Some("install Poppler (pdftotext)".to_string()),
        },
    }
}

/// 当前系统检测到的包管理器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageManager {
    Brew,
    Apt,
    Dnf,
    Apk,
    Winget,
    Unknown,
}

fn detect_package_manager() -> PackageManager {
    let candidates = [
        ("brew", PackageManager::Brew),
        ("apt-get", PackageManager::Apt),
        ("dnf", PackageManager::Dnf),
        ("apk", PackageManager::Apk),
        ("winget", PackageManager::Winget),
    ];
    for (bin, pm) in candidates {
        if probe_executable(bin) {
            return pm;
        }
    }
    PackageManager::Unknown
}

/// 运行 `install-deps`。`yes` 为 true 时真正执行安装命令，否则只打印。
pub fn run_install_deps(yes: bool) -> Result<()> {
    let pm = detect_package_manager();
    println!(
        "Detected package manager: {}",
        match pm {
            PackageManager::Brew => "Homebrew",
            PackageManager::Apt => "apt",
            PackageManager::Dnf => "dnf",
            PackageManager::Apk => "apk",
            PackageManager::Winget => "winget",
            PackageManager::Unknown => "unknown (falling back to platform defaults)",
        }
    );
    println!();

    let all = [
        Dep::Python,
        Dep::Node,
        Dep::Pandoc,
        Dep::Tesseract,
        Dep::Poppler,
    ];

    let mut pending = Vec::new();

    for dep in all {
        if dep.is_present() {
            println!("  ✓ {} present", dep.label());
            continue;
        }
        let status = if dep.required() {
            "✗ missing (required for some tools)"
        } else {
            "· missing (optional)"
        };
        println!("  {} {}", status, dep.label());
        match dep.install_command(pm) {
            Some(cmd) => {
                pending.push(cmd.clone());
                if yes {
                    println!("    Running: {cmd}");
                    match Command::new("sh").arg("-c").arg(&cmd).status() {
                        Ok(s) if s.success() => println!("    ✓ installed"),
                        Ok(s) => println!("    ✗ install exited with {s}"),
                        Err(e) => println!("    ✗ failed to run install: {e}"),
                    }
                } else {
                    println!("    To install: {cmd}");
                }
            }
            None => println!(
                "    No automated installer for this platform; install {} manually.",
                dep.label()
            ),
        }
    }

    println!();
    if yes {
        if pending.is_empty() {
            println!("All dependencies already satisfied.");
        } else {
            println!(
                "Ran {} install command(s). Re-run `mimofan doctor` to confirm.",
                pending.len()
            );
        }
    } else {
        println!("(dry-run) pass --yes to actually run the install commands above.");
    }

    Ok(())
}
