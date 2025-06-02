import asyncio
import time

import opuslib_next
import sounddevice as sd

from audio import sendAudio
from udp import create_udp
import util
import numpy as np


async def send():
    datas, _ = util.audio_to_data("./assets/中秋月.mp3")
    # datas, _ = util.audio_to_data("./assets/wificonfig.p3");
    conn = await create_udp(remote_addr=("172.20.10.7", 8080))
    await sendAudio(conn, datas)


async def receive():
    udp = await create_udp(local_addr=("172.20.10.8", 8080))
    dec = opuslib_next.Decoder(16000, 1)
    print("start receiving")
    stream = sd.OutputStream(
            samplerate=16000,
            channels=1,
            dtype='int16'
        )
    stream.start()

    while True:
        data, addr = await udp.recv()
        print(f"Received {len(data)} bytes from {addr}")
        data = dec.decode(data, 960)
        stream.write(np.frombuffer(data, dtype=np.int16))

asyncio.run(receive())
time.sleep(1000)

import http.server
import ssl
import sys

def run_https_server(bind_addr='0.0.0.0', port=443, certfile='cert.pem', keyfile='key.pem'):
    """
    Starts a basic HTTPS server serving the current directory.

    Args:
        bind_addr (str): address to bind to ('' or '0.0.0.0' for all interfaces).
        port (int): TCP port to listen on.
        certfile (str): path to the PEM-encoded certificate.
        keyfile (str): path to the PEM-encoded private key.
    """
    # 1. Create an HTTPServer instance
    server_address = (bind_addr, port)
    handler_class = http.server.SimpleHTTPRequestHandler
    httpd = http.server.HTTPServer(server_address, handler_class)

    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certfile=certfile, keyfile=keyfile)
    # 2. Wrap its socket with SSL
    httpd.socket = context.wrap_socket(
        httpd.socket,
        server_side=True,
    )

    print(f"Serving HTTPS on {bind_addr or '0.0.0.0'} port {port} (https://{bind_addr or 'localhost'}:{port}/) …")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nKeyboard interrupt received, shutting down.")
        httpd.server_close()
        sys.exit(0)

# run_https_server()
