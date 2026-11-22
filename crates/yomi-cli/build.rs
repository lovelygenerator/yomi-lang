//! The build script. Checks the translation files and generates typed functions.

fn main() {
    lingokit_build::Config::new("locales").generate();
}
