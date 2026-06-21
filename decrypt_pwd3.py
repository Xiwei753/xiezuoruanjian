import base64
import ctypes
import ctypes.wintypes
import re

class DATA_BLOB(ctypes.Structure):
    _fields_ = [
        ('cbData', ctypes.wintypes.DWORD),
        ('pbData', ctypes.POINTER(ctypes.c_char))
    ]

def dpapi_decrypt(encrypted_bytes):
    blob_in = DATA_BLOB()
    blob_in.cbData = len(encrypted_bytes)
    blob_in.pbData = ctypes.create_string_buffer(encrypted_bytes, len(encrypted_bytes))
    blob_out = DATA_BLOB()
    
    if ctypes.windll.crypt32.CryptUnprotectData(
        ctypes.byref(blob_in),
        None,
        None,
        None,
        None,
        0,
        ctypes.byref(blob_out)
    ):
        data = ctypes.string_at(blob_out.pbData, blob_out.cbData)
        ctypes.windll.kernel32.LocalFree(blob_out.pbData)
        return data
    else:
        raise Exception('DPAPI decrypt failed')

# c.pwd中的value
value_b64 = 'AQAAANCMnd8BFdERjHoAwE/Cl+sBAAAAEDfIGrT5bkaS6R9FExLPigAAAAAWAAAATQBhAHMAdABlAHIAIABLAGUAeQAAABBmAAAAAQAAIAAAAAbPsIX2NV79rvTU5/Xc7WwY3gpAlvbRF956bQkW/jL9AAAAAA6AAAAAAgAAIAAAAIJNKgdDlq+TmMIcl1x5gE8B/y3fwWe6Nw6YGy+ceqzc0AIAAJu7w52lSj90HLIPlwHA6TU//rafqra6JeMLbq7UVplYO/S5O67S5mXypXjnTdOXHFqy+g6+Mkq1q4HevcZj14+IcLgvWDfOCY0k60zxjcZdHn8riTkzo0Rm1HK6t1WfoWb7YNonUC2PuaQnofZlR8rMn0KZnQe/6s6f7Ax4HrQWK3xPNWCpfwyi478hdLX1fFebLmPYxT0PfTyyGTxiLjqx0qKP/iZq1uvkung62VaEV1P/K+6ivnI0SeQ+Gb9gcrRX+MLTBI/OAbGDzl97WLPj/3CEfHFL7+lyQQHuf0oXZPj2RCr9Xbh3/JDr4hnKTk3RvFRS3xjs5Osn/BQGMmTLK0Exf4WZJAFxD1lXGz4mx4qGALtmQPpnTRiWH27/C3UWU+vicr9yW6St7NZJtm8uO/MisHKjSvstJi5HFMHnQcC7HTPCtU/MqHANsZCamSKGg13ERALwZDNAO+JFwvp0LdjXFNmj3+SACwpTSStvClI1mtiDW/lUWn2Fep/L6GzlzmIhmB3UeFguTCyN8TOFhRHLFY472uWIdTLRlKaf8BB3SCTtC6z9IhTXjw0kqElJoQ1qJFVTT0+TVg6FLEqltLbkPj5mhx59B/GTLNofwM+EFmGf59KZaC9moXNtbudqQsEdhQCTQggu2NBgRzOm844Bg5ZInlRBUeMXejCGdQrl3n19flNDFyS1ezBAXRhH0biEX/WT6+BcNQWOBDbB8OwqEmzkAa1oiHN+jVrBAPmYcDW3oezqbymXfatjoPCiKVOQoFYkcfQxVIDenFgVuDDlahqG0t9YR1W28ux0CrZ0qG8O+ubMGBvLTcMizi7er42paUGueRLWZ4ED5X84F/mXVpQPr1zrG9Agq598okmJ2M+caEDyYf97IEqmnUbu/t63uNoCCrQZrcVlexxQ3bhuu9lUhgQsuavM6rX/EWQogXF0wAqSEMfJZ98Y9kAAAAAXuXqoV98HC4H6wWeCzpNrAElPuflGNKMEDEECcF3Nj4v+Blp7zH3C1V1D4i/NxLuVxFBVcwSYlsSNrqz3fopM'

data = base64.b64decode(value_b64)
print(f'First 4 bytes: {data[:4].hex()}')
is_dpapi = data[:4] == bytes.fromhex('01000000')
print(f'Is DPAPI blob: {is_dpapi}')
print(f'Total length: {len(data)}')

# DPAPI解密
decrypted = dpapi_decrypt(data)
print(f'Decrypted length: {len(decrypted)}')
print(f'First 20 bytes hex: {decrypted[:20].hex()}')

# 搜索可读的ASCII字符串
ascii_strings = re.findall(b'[\x20-\x7e]{4,}', decrypted)
for s in ascii_strings:
    print(f'ASCII string: {s.decode("ascii")}')

# 搜索UTF-16LE编码的字符串
utf16_strings = re.findall(b'(?:[\x20-\x7e]\x00){3,}', decrypted)
for s in utf16_strings:
    print(f'UTF-16LE string: {s.decode("utf-16-le")}')

# 检查是否是KeePass格式
# KDBX4 signature: 0x9AA2D903
if decrypted[:4] == b'\x03\xd9\xa2\x9a':
    print('This is a KeePass KDBX4 database!')

# 检查是否是Java KeyStore格式
# JKS magic: 0xFEEDFEED
if decrypted[:4] == b'\xfe\xed\xfe\xed':
    print('This is a Java KeyStore!')

# 打印前100字节的hex
print(f'First 100 bytes hex: {decrypted[:100].hex()}')