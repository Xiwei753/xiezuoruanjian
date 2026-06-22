from Crypto.Cipher import AES
from Crypto.Protocol.KDF import PBKDF2
from Crypto.Hash import SHA1, HMAC
import base64
import ctypes
import ctypes.wintypes

# Step 1: 解密c.pwd获取master password
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

encrypted = base64.b64decode(value_b64)
master_key_data = dpapi_decrypt(encrypted)

print(f'Master key data length: {len(master_key_data)}')
print(f'Master key data first 20 bytes hex: {master_key_data[:20].hex()}')

# Step 2: 尝试用不同的master password解密build-profile.json5中的密码
# IntelliJ PasswordSafe加密格式 (0000001B版本):
# 去掉前8个hex字符 (0000001B)
# 剩余hex解码为字节

store_password_encrypted = '20D1A538BCEA2F2934125CB51FE66D2362001B469CC80F5657698897091F83AE78002E6367AB1D'
key_password_encrypted = '0D7991BA59449BD34156D20B72291FA296C6107B7C630C51FC7AC618854707746E1BE3044CD2C9'

def try_decrypt_intellij_password(encrypted_hex, master_password_bytes):
    """尝试用IntelliJ PasswordSafe格式解密密码"""
    encrypted_bytes = bytes.fromhex(encrypted_hex)
    
    # 尝试不同的解析方式
    # 方式1: salt(8) + iv(16) + ciphertext
    if len(encrypted_bytes) > 24:
        salt = encrypted_bytes[:8]
        iv = encrypted_bytes[8:24]
        ciphertext = encrypted_bytes[24:]
    else:
        # 密文太短，尝试其他方式
        # 方式2: salt(16) + iv(16) + ciphertext
        if len(encrypted_bytes) > 32:
            salt = encrypted_bytes[:16]
            iv = encrypted_bytes[16:32]
            ciphertext = encrypted_bytes[32:]
        else:
            print(f'  Encrypted data too short: {len(encrypted_bytes)} bytes')
            return None
    
    # 尝试不同的PBKDF2参数
    for iterations in [100000, 10000, 1000, 1]:
        for dkLen in [16, 32]:
            try:
                key = PBKDF2(master_password_bytes, salt, dkLen=dkLen, count=iterations,
                           prf=lambda p, s: HMAC.new(p, s, SHA1).digest())
                cipher = AES.new(key, AES.MODE_CBC, iv)
                decrypted = cipher.decrypt(ciphertext)
                
                # 检查PKCS5填充
                if len(decrypted) > 0:
                    pad_len = decrypted[-1]
                    if 1 <= pad_len <= 16 and all(b == pad_len for b in decrypted[-pad_len:]):
                        result = decrypted[:-pad_len]
                        try:
                            text = result.decode('utf-8')
                            print(f'  SUCCESS! iterations={iterations}, dkLen={dkLen}')
                            print(f'  Decrypted password: [{text}]')
                            return text
                        except:
                            pass
            except Exception as e:
                pass
    
    return None

# 尝试用DPAPI解密后的master key数据作为密码
print('\nTrying with DPAPI-decrypted master key data...')

# 尝试不同的master password来源
master_passwords = [
    master_key_data,  # 原始DPAPI解密结果
    master_key_data[4:],  # 跳过前4字节
    master_key_data[:16],  # 前16字节
    master_key_data[:32],  # 前32字节
]

for i, mp in enumerate(master_passwords):
    print(f'\nMaster password variant {i} (length={len(mp)}):')
    result = try_decrypt_intellij_password(store_password_encrypted, mp)
    if result:
        print(f'Store password: {result}')
        break

# 也尝试一些简单的密码
print('\nTrying simple master passwords...')
for pwd_str in ['', 'password', 'changeit', 'intellij', 'deveco', 'debug']:
    pwd = pwd_str.encode('utf-8')
    print(f'Master password: [{pwd_str}]')
    result = try_decrypt_intellij_password(store_password_encrypted, pwd)
    if result:
        print(f'Store password: {result}')
        break