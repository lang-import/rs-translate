extern crate redis;
extern crate clap;
extern crate rouille;

use rouille::{Request, Response};
use std::{process, str};
use clap::{Arg, App};

fn list_engines(command: &str) -> Vec<String> {
    let output = process::Command::new(command)
        .arg("-S")
        .output().expect("list of supported engines").stdout;
    str::from_utf8(&output)
        .unwrap()
        .split_whitespace()
        .map(|x| x.trim())
        .filter(|x| !x.is_empty() && !x.starts_with("*"))
        .map(|x| String::from(x))
        .collect()
}


fn translate_engine(command: &str, engine: &str, lang: &str, word: &str) -> Option<String> {
    let lang_arg: String = ":".to_string() + lang;
    let output = match process::Command::new(command)
        .arg("-e").arg(engine).arg("-b").arg(lang_arg).arg(word)
        .output() {
        Ok(v) => v.stdout,
        Err(_) => return None
    };
    if output.is_empty() {
        return None;
    }
    match String::from_utf8(output) {
        Ok(v) => Some(v),
        Err(_) => None
    }
}

fn translate(command: &str, engines: &[String], lang: &str, word: &str) -> Option<String> {
    for engine in engines {
        match translate_engine(&command, &engine.as_str(), &lang, &word) {
            Some(v) => return Some(v),
            None => {}
        };
    }
    None
}

fn translate_cached(connection: &redis::Connection, command: &str, engines: &[String], lang: &str, word: &str) -> Option<String> {
    let value = match redis::cmd("HGET").arg(lang).arg(word).query(connection) {
        Err(e) => {
            println!("failed access the cache: {}", e);
            translate(&command, &engines, &lang, &word)
        }
        Ok(value) => {
            match value {
                Some(v) => return v,
                None => translate(&command, &engines, &lang, &word)
            }
        }
    };
    match value {
        Some(v) => {
            match redis::cmd("HSET").arg(lang).arg(word).arg(v.as_str()).query(connection) {
                Err(e) => println!("failed save to cache word {} for lang {}: {}", word, lang, e),
                Ok(()) => {}
            };
            Some(v)
        }
        None => None
    }
}

fn main() {
    let matches = App::new("Translate API")
        .version("1.0")
        .author("Alexander Baryshnikov <dev@baryshnikov.net>")
        .about("exposes trans-shell to Web")
        .arg(Arg::with_name("binary")
            .short("b")
            .long("bin")
            .help("path to binary for translate-shell")
            .default_value("/usr/bin/trans")
            .takes_value(true))
        .arg(Arg::with_name("redis")
            .short("r")
            .long("redis")
            .help("redis URL")
            .default_value("redis://127.0.0.1/")
            .takes_value(true))
        .arg(Arg::with_name("address")
            .short("a")
            .long("address")
            .help("binding address")
            .default_value("127.0.0.1:8000")
            .takes_value(true))
        .get_matches();


    let binding_addr = matches.value_of("address").unwrap();
    let command: String = matches.value_of("binary").unwrap().to_string();
    let redis_url = matches.value_of("redis").unwrap();
    let client = redis::Client::open(redis_url).expect("connect to redis");
    let engines = list_engines(command.as_str());
    for engine in &engines {
        println!("found engine {}", engine)
    }
    println!("started server on {}", binding_addr);

    rouille::start_server(binding_addr, move |request: &Request| {
        let u = request.url();
        let segments: Vec<&str> = u.as_str().split("/").collect();
        if segments.len() != 5 || !(segments[1] == "translate" && segments[3] == "to") {
            return Response::text("bad request").with_status_code(422);
        }
        let word = segments[2];
        let lang = segments[4];

        let connection = client.get_connection().expect("open connection to redis");
        let ans = translate_cached(&connection, command.as_str(), &engines, lang, word).unwrap();
        Response::text(ans)
    });
}
