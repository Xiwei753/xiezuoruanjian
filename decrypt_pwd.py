import base64
import ctypes
import ctypes.wintypes

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
        error = ctypes.GetLastError()
        raise Exception(f'DPAPI decrypt failed, error: {error}')

# build-profile.json5中的storePassword
# 格式: 0000001B + hex(AES_encrypted_password)
encrypted_hex = '20D1A538BCEA2F2934125CB51FE66D2362001B469CC80F5657698897091F83AE78002E6367AB1D'
encrypted_bytes = bytes.fromhex(encrypted_hex)

# 尝试直接用DPAPI解密
try:
    decrypted = dpapi_decrypt(encrypted_bytes)
    print('DPAPI direct decrypt succeeded!')
    print(f'Decrypted bytes: {decrypted.hex()}')
    try:
        text = decrypted.decode('utf-8')
        print(f'UTF-8: [{text}]')
    except:
        pass
    try:
        text = decrypted.decode('utf-16-le')
        print(f'UTF-16LE: [{text}]')
    except:
        pass
except Exception as e:
    print(f'DPAPI direct decrypt failed: {e}')

# 也尝试完整的密码字符串(包括0000001B前缀)
full_hex = '0000001B' + encrypted_hex
full_bytes = bytes.fromhex(full_hex)
try:
    decrypted = dpapi_decrypt(full_bytes)
    print('DPAPI full decrypt succeeded!')
    print(f'Decrypted bytes: {decrypted.hex()}')
    try:
        text = decrypted.decode('utf-8')
        print(f'UTF-8: [{text}]')
    except:
        pass
    try:
        text = decrypted.decode('utf-16-le')
        print(f'UTF-16LE: [{text}]')
    except:
        pass
except Exception as e:
    print(f'DPAPI full decrypt failed: {e}')

# 也尝试keyPassword
key_encrypted_hex = '0D7991BA59449BD34156D20B72291FA296C6107B7C630C51FC7AC618854707746E1BE3044CD2C9'
key_encrypted_bytes = bytes.fromhex(key_encrypted_hex)
try:
    decrypted = dpapi_decrypt(key_encrypted_bytes)
    print('DPAPI keyPassword decrypt succeeded!')
    print(f'Decrypted bytes: {decrypted.hex()}')
    try:
        text = decrypted.decode('utf-8')
        print(f'UTF-8: [{text}]')
    except:
        pass
    try:
        text = decrypted.decode('utf-16-le')
        print(f'UTF-16LE: [{text}]')
    except:
        pass
except Exception as e:
    print(f'DPAPI keyPassword decrypt failed: {e}')