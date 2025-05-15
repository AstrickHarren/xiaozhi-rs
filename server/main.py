import asyncio
import socket
import time
import sounddevice
import numpy as np

import opuslib_next
from audio import sendAudio
from udp import create_udp
import util
from sounddevice import play
import audioop

datas, _ = util.audio_to_data("./assets/中秋月.mp3")

decoded = b''
decoder = opuslib_next.Decoder(16000, 1)
for data in datas:
    audio = decoder.decode(data, 960)
    decoded += audio
print(len(decoded))
play(np.frombuffer(decoded, dtype=np.int16), samplerate=16000)


# buf = np.frombuffer(decoded, dtype=np.int16)
# def callback(out, frames, time, status):
#     global buf
#     out[:, 0] = buf[:frames]
#     buf = buf[frames:]
# stream = sounddevice.OutputStream(samplerate=16000, channels=1, callback=callback)
# stream.start()

time.sleep(1)
