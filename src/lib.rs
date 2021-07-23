use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub struct Opt {
    /// Subnet to search.
    #[structopt(name = "subnet")]
    pub subnet: String,
}
