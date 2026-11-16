// 실행 파일에 아이콘 · 버전 정보 넣기 — assets/tempo-res.o (tools/make_res.py 로 만든 COFF 리소스)를 링크
fn main() {
    let obj = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("tempo-res.o");
    println!("cargo:rerun-if-changed={}", obj.display());
    let target = std::env::var("TARGET").unwrap_or_default();
    if obj.exists() && target.contains("windows-gnu") {
        println!("cargo:rustc-link-arg-bins={}", obj.display());
    }
}
