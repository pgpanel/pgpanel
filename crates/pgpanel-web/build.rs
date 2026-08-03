//! Fail the build if frontend static assets are missing or unstyled.

const CSS_PATH: &str = "../../static/css/app.css";
const JS_PATHS: &[&str] = &[
    "../../static/js/app.js",
    "../../static/js/htmx.min.js",
    "../../static/js/alpine.min.js",
];
const MIN_CSS_BYTES: u64 = 5_000;

fn main() {
    println!("cargo:rerun-if-changed={CSS_PATH}");
    for path in JS_PATHS {
        println!("cargo:rerun-if-changed={path}");
    }

    let css = std::path::Path::new(CSS_PATH);
    if !css.exists() {
        panic!(
            "missing {path}: compiled Tailwind CSS is required.\n\
             Run `./scripts/build-frontend.sh` or `make frontend` before building pgpanel-web.",
            path = CSS_PATH
        );
    }

    let size = std::fs::metadata(css)
        .map_err(|e| format!("failed to read {CSS_PATH}: {e}"))
        .unwrap()
        .len();

    if size < MIN_CSS_BYTES {
        panic!(
            "{path} is too small ({size} bytes; expected at least {min}).\n\
             Run `./scripts/build-frontend.sh` or `make frontend` to compile Tailwind CSS.",
            path = CSS_PATH,
            size = size,
            min = MIN_CSS_BYTES
        );
    }
}
