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
