from isort import file
import os
import numpy as np
import opuslib_next
from pydub import AudioSegment
import struct

def raw_data(audio_file_path):
    file_type = os.path.splitext(audio_file_path)[1]
    if file_type:
        file_type = file_type.lstrip(".")
    # 读取音频文件，-nostdin 参数：不要从标准输入读取数据，否则FFmpeg会阻塞
    audio = AudioSegment.from_file(
        audio_file_path, format=file_type, parameters=["-nostdin"]
    )
    # 转换为单声道/16kHz采样率/16位小端编码（确保与编码器匹配）
    audio = audio.set_channels(1).set_frame_rate(16000).set_sample_width(2)
    # 获取原始PCM数据（16位小端）
    raw_data = audio.raw_data
    return raw_data


def audio_to_data(audio_file_path, is_opus=True):
    # 获取文件后缀名
    file_type = os.path.splitext(audio_file_path)[1]
    if file_type:
        file_type = file_type.lstrip(".")

    if file_type == "p3":
        return decode_opus_from_file(audio_file_path)

    # 读取音频文件，-nostdin 参数：不要从标准输入读取数据，否则FFmpeg会阻塞
    audio = AudioSegment.from_file(
        audio_file_path, format=file_type, parameters=["-nostdin"]
    )

    # 转换为单声道/16kHz采样率/16位小端编码（确保与编码器匹配）
    audio = audio.set_channels(1).set_frame_rate(16000).set_sample_width(2)

    # 音频时长(秒)
    duration = len(audio) / 1000.0

    # 获取原始PCM数据（16位小端）
    raw_data = audio.raw_data

    # 初始化Opus编码器
    encoder = opuslib_next.Encoder(16000, 1, opuslib_next.APPLICATION_AUDIO)

    # 编码参数
    frame_duration = 60  # 60ms per frame
    frame_size = int(16000 * frame_duration / 1000)  # 960 samples/frame

    datas = []
    # 按帧处理所有音频数据（包括最后一帧可能补零）
    for i in range(0, len(raw_data), frame_size * 2):  # 16bit=2bytes/sample
        # 获取当前帧的二进制数据
        chunk = raw_data[i : i + frame_size * 2]

        # 如果最后一帧不足，补零
        if len(chunk) < frame_size * 2:
            chunk += b"\x00" * (frame_size * 2 - len(chunk))

        if is_opus:
            # 转换为numpy数组处理
            np_frame = np.frombuffer(chunk, dtype=np.int16)
            # 编码Opus数据
            frame_data = encoder.encode(np_frame.tobytes(), frame_size)
        else:
            frame_data = chunk if isinstance(chunk, bytes) else bytes(chunk)

        datas.append(frame_data)

    return datas, duration

def decode_opus_from_file(input_file):
    """
    从p3文件中解码 Opus 数据，并返回一个 Opus 数据包的列表以及总时长。
    """
    opus_datas = []
    total_frames = 0
    sample_rate = 16000  # 文件采样率
    frame_duration_ms = 60  # 帧时长
    frame_size = int(sample_rate * frame_duration_ms / 1000)

    with open(input_file, 'rb') as f:
        while True:
            # 读取头部（4字节）：[1字节类型，1字节保留，2字节长度]
            header = f.read(4)
            if not header:
                break

            # 解包头部信息
            _, _, data_len = struct.unpack('>BBH', header)
            print("data_len: ", data_len)

            # 根据头部指定的长度读取 Opus 数据
            opus_data = f.read(data_len)
            if len(opus_data) != data_len:
                raise ValueError(f"Data length({len(opus_data)}) mismatch({data_len}) in the file.")

            opus_datas.append(opus_data)
            total_frames += 1

    # 计算总时长
    total_duration = (total_frames * frame_duration_ms) / 1000.0
    return opus_datas, total_duration
