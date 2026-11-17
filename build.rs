//! Embeds the icon and the version info (the name Task Manager and Explorer
//! show) into the exe.
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    let version = env!("CARGO_PKG_VERSION");
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/app.ico");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Simple PC Tools"
      VALUE "ProductName", "Simple PC Tools"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "GPL-3.0-or-later"
      VALUE "OriginalFilename", "simple-pc-tools.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
