#[cfg(windows)]
extern crate embed_resource;

#[cfg(windows)]
fn main() {
    embed_resource::compile("resources/resources.rc");
}

#[cfg(not(windows))]
fn main() {}
