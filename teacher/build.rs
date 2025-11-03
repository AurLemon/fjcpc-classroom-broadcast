fn main() {
    if cfg!(target_os = "windows") {
        embed_resource::compile("resource.rc", embed_resource::NONE);
    }
}
