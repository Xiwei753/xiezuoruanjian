// PeerCrypto — 附近设备对象传输的字节/哈希工具（Issue #988）。
//
// 第三方跨设备连接只保证文本消息，因此正文按 UTF-8 字节做 Base64 分片；
// 完整性使用 SHA-256，而不是仅凭 32 位短哈希判断对象是否一致。

import util from '@ohos.util'

const SHA256_K: number[] = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
]

function rotr(value: number, bits: number): number {
  return (value >>> bits) | (value << (32 - bits))
}

export class PeerCrypto {
  static utf8Encode(text: string): Uint8Array {
    const bytes: number[] = []
    for (let i = 0; i < text.length; i++) {
      let code: number = text.charCodeAt(i)
      if (code >= 0xd800 && code <= 0xdbff && i + 1 < text.length) {
        const next: number = text.charCodeAt(i + 1)
        if (next >= 0xdc00 && next <= 0xdfff) {
          code = 0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00)
          i++
        }
      }
      if (code < 0x80) {
        bytes.push(code)
      } else if (code < 0x800) {
        bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f))
      } else if (code < 0x10000) {
        bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f))
      } else {
        bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 0x3f),
          0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f))
      }
    }
    return new Uint8Array(bytes)
  }

  static utf8Decode(bytes: Uint8Array): string {
    let result: string = ''
    let i: number = 0
    while (i < bytes.length) {
      const first: number = bytes[i++]
      let code: number = 0xfffd
      if (first < 0x80) {
        code = first
      } else if ((first & 0xe0) === 0xc0 && i < bytes.length) {
        code = ((first & 0x1f) << 6) | (bytes[i++] & 0x3f)
      } else if ((first & 0xf0) === 0xe0 && i + 1 < bytes.length) {
        code = ((first & 0x0f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f)
      } else if ((first & 0xf8) === 0xf0 && i + 2 < bytes.length) {
        code = ((first & 0x07) << 18) | ((bytes[i++] & 0x3f) << 12)
          | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f)
      }
      if (code > 0xffff) {
        code -= 0x10000
        result += String.fromCharCode(0xd800 + (code >> 10), 0xdc00 + (code & 0x3ff))
      } else {
        result += String.fromCharCode(code)
      }
    }
    return result
  }

  // 现有 manifest.sync.json/known_files 使用 MD5；传输完整性仍使用 SHA-256。
  static md5Hex(bytes: Uint8Array): string {
    const shifts: number[] = [7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
      5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
      4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
      6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21]
    const k: number[] = []
    for (let i = 0; i < 64; i++) {
      k.push(Math.floor(Math.abs(Math.sin(i + 1)) * 0x100000000) | 0)
    }
    const bitLength: number = bytes.length * 8
    const paddedLength: number = (((bytes.length + 9 + 63) >> 6) << 6)
    const padded: Uint8Array = new Uint8Array(paddedLength)
    padded.set(bytes)
    padded[bytes.length] = 0x80
    const view: DataView = new DataView(padded.buffer)
    view.setUint32(paddedLength - 8, bitLength >>> 0, true)
    view.setUint32(paddedLength - 4, Math.floor(bitLength / 0x100000000), true)

    let a0: number = 0x67452301
    let b0: number = 0xefcdab89
    let c0: number = 0x98badcfe
    let d0: number = 0x10325476
    const m: number[] = new Array<number>(16).fill(0)
    for (let offset = 0; offset < paddedLength; offset += 64) {
      for (let i = 0; i < 16; i++) {
        m[i] = view.getUint32(offset + i * 4, true)
      }
      let a: number = a0
      let b: number = b0
      let c: number = c0
      let d: number = d0
      for (let i = 0; i < 64; i++) {
        let f: number
        let g: number
        if (i < 16) {
          f = (b & c) | ((~b) & d)
          g = i
        } else if (i < 32) {
          f = (d & b) | ((~d) & c)
          g = (5 * i + 1) % 16
        } else if (i < 48) {
          f = b ^ c ^ d
          g = (3 * i + 5) % 16
        } else {
          f = c ^ (b | (~d))
          g = (7 * i) % 16
        }
        const sum: number = (a + f + k[i] + m[g]) | 0
        const rotated: number = ((sum << shifts[i]) | (sum >>> (32 - shifts[i]))) | 0
        a = d
        d = c
        c = b
        b = (b + rotated) | 0
      }
      a0 = (a0 + a) | 0
      b0 = (b0 + b) | 0
      c0 = (c0 + c) | 0
      d0 = (d0 + d) | 0
    }
    return [a0, b0, c0, d0].map((value: number): string => {
      let hex: string = ''
      for (let i = 0; i < 4; i++) {
        hex += ((value >>> (i * 8)) & 0xff).toString(16).padStart(2, '0')
      }
      return hex
    }).join('')
  }

  static sha256Hex(bytes: Uint8Array): string {
    const bitLength: number = bytes.length * 8
    const paddedLength: number = (((bytes.length + 9 + 63) >> 6) << 6)
    const padded: Uint8Array = new Uint8Array(paddedLength)
    padded.set(bytes)
    padded[bytes.length] = 0x80
    const view: DataView = new DataView(padded.buffer)
    view.setUint32(paddedLength - 8, Math.floor(bitLength / 0x100000000), false)
    view.setUint32(paddedLength - 4, bitLength >>> 0, false)

    let h0: number = 0x6a09e667
    let h1: number = 0xbb67ae85
    let h2: number = 0x3c6ef372
    let h3: number = 0xa54ff53a
    let h4: number = 0x510e527f
    let h5: number = 0x9b05688c
    let h6: number = 0x1f83d9ab
    let h7: number = 0x5be0cd19
    const w: number[] = new Array<number>(64).fill(0)

    for (let offset = 0; offset < paddedLength; offset += 64) {
      for (let i = 0; i < 16; i++) {
        w[i] = view.getUint32(offset + i * 4, false)
      }
      for (let i = 16; i < 64; i++) {
        const s0: number = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3)
        const s1: number = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10)
        w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0
      }
      let a: number = h0
      let b: number = h1
      let c: number = h2
      let d: number = h3
      let e: number = h4
      let f: number = h5
      let g: number = h6
      let h: number = h7
      for (let i = 0; i < 64; i++) {
        const s1: number = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)
        const ch: number = (e & f) ^ ((~e) & g)
        const temp1: number = (h + s1 + ch + SHA256_K[i] + w[i]) | 0
        const s0: number = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)
        const maj: number = (a & b) ^ (a & c) ^ (b & c)
        const temp2: number = (s0 + maj) | 0
        h = g
        g = f
        f = e
        e = (d + temp1) | 0
        d = c
        c = b
        b = a
        a = (temp1 + temp2) | 0
      }
      h0 = (h0 + a) | 0
      h1 = (h1 + b) | 0
      h2 = (h2 + c) | 0
      h3 = (h3 + d) | 0
      h4 = (h4 + e) | 0
      h5 = (h5 + f) | 0
      h6 = (h6 + g) | 0
      h7 = (h7 + h) | 0
    }
    return [h0, h1, h2, h3, h4, h5, h6, h7]
      .map((value: number): string => (value >>> 0).toString(16).padStart(8, '0'))
      .join('')
  }

  static sha256Text(text: string): string {
    return PeerCrypto.sha256Hex(PeerCrypto.utf8Encode(text))
  }

  static base64Encode(bytes: Uint8Array): string {
    return new util.Base64Helper().encodeToStringSync(bytes)
  }

  static base64Decode(text: string): Uint8Array {
    return new util.Base64Helper().decodeSync(text)
  }
}
