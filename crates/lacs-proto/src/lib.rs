pub mod lacs {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/lacs.v1.rs"));
    }
}
