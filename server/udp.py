import asyncudp

async def create_udp(local_addr=None, remote_addr=None):
    sock = await asyncudp.create_socket(
        local_addr=local_addr,
        remote_addr=remote_addr,
        reuse_port=True)
    return UdpConn(sock)

class UdpConn:
    def __init__(self, sock: asyncudp.Socket):
        self.sock = sock
        self.client_abort = False

    async def send(self, data):
        self.sock.sendto(data)

    async def recv(self):
        return await self.sock.recvfrom()
