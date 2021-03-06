#![feature(proc_macro)]
#![feature(plugin)]
#![feature(conservative_impl_trait)]
#![plugin(bifrost_plugins)]

#![feature(conservative_impl_trait, generators)]

#[macro_use]
extern crate neb;
#[macro_use]
extern crate hivemind;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate bifrost;
#[macro_use]
extern crate bifrost_hasher;
extern crate futures_await as futures;
extern crate futures_cpupool;
extern crate parking_lot;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate chashmap;
#[macro_use]
extern crate log;
extern crate log4rs;
extern crate env_logger;
extern crate yaml_rust;
extern crate serde_yaml;

use futures::Future;

mod graph;
mod server;
mod utils;
mod config;
mod query;
#[cfg(test)]
mod tests;

use std::thread;

fn main() {
    log4rs::init_file("config/log4rs.yaml", Default::default()).unwrap();
    info!("Shisoft Morpheus is initializing...");
    query::init().unwrap();
    let neb_config = config::neb::options_from_file("config/neb.yaml");
    let morpheus_server = server::MorpheusServer::new(neb_config).wait().unwrap();

    thread::park();
}
