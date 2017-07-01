#![feature(proc_macro)]
#![feature(plugin)]
#![plugin(bifrost_plugins)]

extern crate neb;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate bifrost;
#[macro_use]
extern crate bifrost_hasher;
extern crate futures;
extern crate parking_lot;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate chashmap;
#[macro_use]
extern crate log;
extern crate env_logger;

mod graph;
mod server;
mod utils;

fn main() {
    println!("Hello, world!");
}
