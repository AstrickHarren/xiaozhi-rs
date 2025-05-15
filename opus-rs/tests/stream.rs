use opus::Decoder;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, UdpSocket},
};

#[tokio::test]
async fn test_stream() {
    let mut sock = UdpSocket::bind("0.0.0.0:8080").await.unwrap();
    // let mut stream = sock.connect("127.0.0.1:8080").await.unwrap();

    let mut dec = Decoder::new(16000, opus::Channels::Stereo).unwrap();

    //stream.write_all(b"GET /test.opus HTTP/1.0\r\nHost: 192.168.31.172\r\n\r\n").await.unwrap();
    //stream.flush().await.unwrap();

    let mut buf = [0; 1024];
    let mut pcm = [0; 4096];
    loop {
        match sock.recv(&mut buf).await {
            Ok(n) if n > 0 => {
                //let buf = &buf[..n];
                //println!("buf {:?}", buf);
                //println!("udp recved {n} bytes");
                let buf = &buf[..n];
                let x =
                    dec.decode(&buf, &mut pcm, false).inspect_err(|e| println!("{e:?}")).unwrap();

                let mut pcm: Vec<_> =
                    pcm.into_iter().take(n * 2).flat_map(|x| x.to_le_bytes()).collect();
                println!("{:?}", &buf);
                println!("{:?}", &pcm[..200.min(pcm.len())]);
                println!("\n\n\n");
            }
            _ => break,
        }
    }
    println!("closing")
}
