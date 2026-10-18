//! Windows 실행 파일에 아이콘과 버전 정보를 넣는다(탐색기의 속성 창, 작업 표시줄).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "LDF Studio")
            .set(
                "FileDescription",
                "LDF Studio - LIN Description File 뷰어·검사기·편집기",
            )
            .set("OriginalFilename", "ldf-studio.exe")
            .set("ProductVersion", &version)
            .set("FileVersion", &version);
        // 리소스 컴파일러가 없는 환경에서도 빌드는 되게 한다(아이콘만 빠진다)
        if let Err(e) = res.compile() {
            println!("cargo:warning=리소스를 넣지 못했다: {e}");
        }
    }
}
