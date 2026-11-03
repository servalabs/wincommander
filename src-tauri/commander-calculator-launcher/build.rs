use std::{env, fs, path::PathBuf};

fn resource_path(path: PathBuf) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let image = root.join("../commander-free/icons/calc.png");
    println!("cargo:rerun-if-changed={}", image.display());
    println!("cargo:rerun-if-changed=app.manifest");
    let png = fs::read(image).expect("read existing Calculator icon");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 256);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 256);
    let mut icon = vec![0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 32, 0];
    icon.extend_from_slice(&(png.len() as u32).to_le_bytes());
    icon.extend_from_slice(&22u32.to_le_bytes());
    icon.extend_from_slice(&png);
    let icon_path = output.join("calculator.ico");
    fs::write(&icon_path, icon).expect("write Calculator resource icon");
    let resource = format!(
        r#"1 ICON "{}"
1 24 "{}"
1 VERSIONINFO
FILEVERSION 3,6,6,0
PRODUCTVERSION 3,6,6,0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "ServaLabs\0"
      VALUE "FileDescription", "Calculator\0"
      VALUE "FileVersion", "3.6.6\0"
      VALUE "InternalName", "Calculator\0"
      VALUE "OriginalFilename", "Calculator.exe\0"
      VALUE "ProductName", "Calculator\0"
      VALUE "ProductVersion", "3.6.6\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        resource_path(icon_path),
        resource_path(root.join("app.manifest")),
    );
    let resource_path = output.join("calculator.rc");
    fs::write(&resource_path, resource).expect("write Calculator version resource");
    embed_resource::compile(resource_path, embed_resource::NONE)
        .manifest_required()
        .expect("embed Calculator asInvoker manifest and resources");
}
