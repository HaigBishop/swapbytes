File Request-Response using libp2p
In this project we will create a simple peer-to-peer application to request a file by name from another peer and the peer will respond with the file's data bytes.

Create a new Rust project:

cargo new libp2p-req-res

Cargo.toml
Add dependencies to Cargo.toml:
 








tokio = { version = "1.44.0", features = ["full"] }
async-trait = "0.1.81"
futures = "0.3.31"
clap = { version = "4.5.32", features = ["derive"] }
serde = { version = "1.0", features = ["derive"] }

[dependencies.libp2p]
version = "0.55"
features = ["tokio", "noise", "macros", "tcp", "quic", "yamux", "cbor", "request-response"]
There are a couple of differences from the chat project. First, a syntactic difference: we use [dependencies.libp2p] in Cargo.toml to decompose the libp2p dependency into its own section. This is particularly useful when we have a lot of features to list and you don't want all the parameters on one line.

We have also added the clap crate which is a nice command-line argument parser for Rust. https://docs.rs/clap/latest/clap/

main.rs
Go to main.rs and remove everything. For simplicity here are all of the modules we will use. Add these to the top of main.rs:















use clap::Parser;
use std::{error::Error, time::Duration};
use libp2p::{
    noise, 
    request_response::{self, ProtocolSupport}, 
    swarm::{NetworkBehaviour, SwarmEvent}, 
    tcp, yamux, Multiaddr, PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use futures::StreamExt;
use tokio::{
    io::{self, stdin, AsyncBufReadExt, BufReader, AsyncReadExt}, 
    select, 
    fs::File
};
Next create the main function:








#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // add main program code here
    // ...
    
    // this line is just here to make it compile. Keep at the bottom of main or you can remove it later.
    Ok(())
}
clap
The first thing we want to set up is the command like parser. For our application we will have two command line parameters:

--port an optional parameter which sets the port number the peer is listening on.
--peer an optional parameter to pass the multiaddr of a file-serving peer. Use this if you are a file-requesting peer.
clap makes it easy to define command line parameters as a struct that uses the Parser derive macro:










// Command line parsing
#[derive(Parser, Debug)]
#[clap(name = "libp2p request response example")]
struct Cli {
    #[arg(long)]
    port: Option<String>,

    #[arg(long)]
    peer: Option<Multiaddr>,
}
The #[arg] attribute allows you to customize how the parameter is parsed (e.g. in short or long form). See the documentation for clap link above for more information on the options available. In this case we are parsing the port parameter as a String and the peer parameter as a Multiaddr.

In fn main() add a line at the top to parse the command line options:




#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // parse the command line options
    let cli = Cli::parse();
You can now run your program and see the command line parser has been set up.

cargo run -- --help

To pass command line parameters to an executable run with cargo run you use -- to indicate that every parameter after that is passed to the program, not cargo. So, the line above is running the default --help command on your program.

We will use the cli variable later on to access the values that the user has passed in.

Setting up swarm
Now let's set up the p2p swarm. We start with defining our network's behaviour. Recall for the chat application the behaviour described the discovery mechanism (mDNS) and the message passing protocol (Gossipsub). For simplicity in this case, we start by just connecting two nodes on the same machine, so we do not need a discovery mechanism behaviour. However, we do need to define a Request-Response behaviour:





// behaviour for the network (there's no discovery in this example)
#[derive(NetworkBehaviour)]
struct ReqResBehaviour {
    request_response: request_response::cbor::Behaviour<FileRequest, FileResponse>,
}
ReqResBehaviour is a behaviour that implements a user-defined request-response protocol. The middle part of this statement, cbor, defines the serialization encoding for the data sent with each request and response. cbor stands for Concise-Binary Object Representation and is a modern standard that is optimized for serializing binary data (https://cbor.io/). We could also have used json, but binary data in JSON must be encoded using Base64 representation which is wasteful. cbor is a better option in this case.

The last two items on that line: FileRequest and FileResponse are protocol structs. They should define what we expect the request and response messages to look like. You might be familiar with protocols such as HTTP. Here, we are not using anything like that. We simply create a custom protocol for our application. To do this we define those two structs as follows:






// file exchange protocol
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRequest(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileResponse(Vec<u8>);
Our protocol is very simple. A request takes the form of an arbitrary String which will represent the file name, and the response is a vector of bytes (Vec<u8>) containing the file data.

With this behaviour defined we can build the swarm in main.



















#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // parse the command line options
    let cli = Cli::parse();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
        .with_behaviour(|_key| ReqResBehaviour {
            request_response: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/file-exchange/1"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(), 
            )
        })?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(7200)))
        .build();
Lines 6-8, 18-19 should be familiar from the chat application. The difference is our call to with_behaviour creates a new ReqResBehaviour with a StreamProtocol we name /file-exchange/1.

Finally, to complete the setup add the following code:











    let listen_port = cli.port.unwrap_or("0".to_string());
    let multiaddr = format!("/ip4/0.0.0.0/tcp/{listen_port}");
    let _ = swarm.listen_on(multiaddr.parse()?)?;

    if let Some(peer) = cli.peer {
        swarm.dial(peer)?;
    }

    let mut stdin: io::Lines<BufReader<io::Stdin>> = BufReader::new(stdin()).lines();

    let mut other_peer_id: Option<PeerId> = None;
Line 1 parses the command line port parameter. If none is given, then it defaults to 0. Lines 2-3 setup the swarm to listen on localhost with the given port number. Lines 5-7 check if a peer command line parameter was given. If so, we dial it to make a connection. Line 9 sets up the async IO reader (same as in chat example). Line 11 creates a mutable Option variable that will store the peer id of the peer we are dialing. We do not know their id until we have established a connection with them, so we set it as None for now.

Event handler loop
Now that we have set up the swarm and prepared the IO, we define an infinite loop that handles IO events and the events in our network.

The full loop looks like this. We will go through each part in the description afterward.







































    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                if let Some(peer_id) = other_peer_id {
                    swarm.behaviour_mut().request_response.send_request(&peer_id, FileRequest(line));
                }
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {address}");
                }
                SwarmEvent::Behaviour(ReqResBehaviourEvent::RequestResponse(
                    request_response::Event::Message { message, .. }
                )) => match message {
                    request_response::Message::Request { request, channel, .. } => {
                        println!("request {:?}", request);
                        let filename = request.0;
                        let file_bytes = match File::open(filename).await {
                            Ok(mut file) => {
                                let mut buffer = Vec::new();
                                file.read_to_end(&mut buffer).await?;
                                buffer
                            }
                            Err(_) => vec![]
                        };
                        let _ = swarm.behaviour_mut().request_response.send_response(channel, FileResponse(file_bytes));
                    }
                    request_response::Message::Response { response, .. } => {
                        println!("response: {:?}", response);
                    }
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    other_peer_id = Some(peer_id);
                    println!("Established connection to {:?}", peer_id);
                }
                _ => { }
            }
        }
    }
Similar to the chat example, Lines 3-7 handle each line of input from stdin. In this case we interpret the input line that was entered as the filename we are requesting from our peer. If the other_peer_id is None nothing happens, but if it is set then we send a FileRequest to the other peer that contains the string that we have entered. This string will be interpreted as the file name by the receiver.

Lines 8-37 describe how we want to respond to various swarm events.

Lines 9-11 simply log that our peer is listening for connections.

Lines 12-31 handle request-reponse event messages. In here is where we handle both request and response events.

If we receive a request from another peer, then we send the response containing the bytes of the file that was requested. On line 17 we get the filename from the request. In lines 18-25 we attempt to open the filename requested. If it exists we read to the end of the file and get the file bytes, otherwise if the file does not exist we simply return an empty Vec of bytes. Finally in line 26, we send the FileResponse on the same channel that we received the request.

Lines 28-30 show how we handle a response from a peer. In this example we simply print out the bytes of the file. We could also save the file to disk here.