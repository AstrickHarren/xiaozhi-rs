import asyncio
import time

import opuslib_next

from audio import sendAudio
from udp import create_udp
import util
import numpy as np


async def main():
    datas, _ = util.audio_to_data("./assets/中秋月.mp3")
    conn = await create_udp(remote_addr=("172.20.10.7", 8080))
    await conn.send(b"hello")
    await sendAudio(conn, datas)


def verify():
    datas, _ = util.audio_to_data("./assets/中秋月.mp3")
    dec = opuslib_next.Decoder(16000, 1)
    for data in datas[:10]:
        print(f"-----{len(data)}-----")
        print(data)
        pcm = dec.decode(data, 960)


# verify()
asyncio.run(main())
# print("sleeping")
time.sleep(1000)
