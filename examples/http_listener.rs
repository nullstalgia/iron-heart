use argh::FromArgs;
use tiny_http::{Response, Server};

/// A simple HTTP server that prints incoming requests.
#[derive(FromArgs)]
struct Args {
    /// port to listen on (default: 9000)
    #[argh(option, default = "9000")]
    port: u16,
}

fn main() {
    let args: Args = argh::from_env();

    let addr = format!("0.0.0.0:{}", args.port);
    let server = Server::http(&addr).expect("Failed to open HTTP Server");

    println!("Listening on {}", addr);

    for mut request in server.incoming_requests() {
        println!("Method: {:?}", request.method());

        println!("Headers:");
        for header in request.headers() {
            println!("  {}: {}", header.field, header.value);
        }

        let mut content = String::new();
        if request.as_reader().read_to_string(&mut content).is_ok() {
            println!("Body:\n{}", content);
        } else {
            println!("Body: [binary data]");
        }

        let response = Response::from_string("OK");
        if let Err(e) = request.respond(response) {
            eprintln!("Failed to send response: {}", e);
        }
    }
}
