fn main() {
    #[cfg(target_os = "freebsd")]
    {
        cc::Build::new()
            .file("src/freebsd.c")
            .compile("ifaddr6_freebsd");
    }
}
