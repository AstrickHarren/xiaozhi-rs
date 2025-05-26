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


def verify():
    datas, _ = util.audio_to_data("./assets/wificonfig.p3")
    dec = opuslib_next.Decoder(16000, 1)
    for data in datas:
        pcm = dec.decode(data, 960)
        print("[ ", end="")
        for i in np.frombuffer(pcm, dtype=np.int16):
            print(i, end=" ")
        print("]")

async def receive():
    udp = await create_udp(local_addr=("172.20.10.8", 8080))
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
        stream.write(np.frombuffer(data, dtype=np.int16))

# verify()
asyncio.run(send())
# print("sleeping")
time.sleep(1000)
