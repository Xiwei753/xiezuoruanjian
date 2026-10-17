/*
 * 素笺 HarmonyOS HUKS 安全存储 C shim 窄接口（Issue #924）。
 *
 * 该头文件只对 Rust 暴露字节缓冲接口，不泄漏 HUKS NDK 的类型（OH_Huks_*）
 * 与会话细节：
 * - 设备级不可导出 AES-256 密钥由 HUKS 托管，别名固定，首次使用时生成；
 * - AES-GCM 加密输出为 `ciphertext || tag`（tag 固定 16 字节，位于末尾）；
 * - 解密要求 nonce 与 tag 分开传入；
 * - nonce 由 shim 从平台随机源（/dev/urandom）读取，失败即报错，不降级。
 *
 * 返回值约定：`0` 表示成功；正数为 HUKS 错误码（OH_HUKS_ERR_CODE_*）；
 * 负数为 shim 本地错误（-1 参数非法 / 缓冲不足，-2 随机源不可用，
 * -3 加解密自检回环不一致）。失败时 `error_out`（若非空）写入 NUL 结尾
 * 的错误描述，供 Rust 侧记录诊断——错误描述中不得包含明文凭据。
 */

#ifndef SUJIAN_HUKS_STORAGE_H
#define SUJIAN_HUKS_STORAGE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* AES-GCM 参数：nonce 长度（字节），AE tag 长度（字节）。 */
#define SUJIAN_HUKS_NONCE_LEN 12u
#define SUJIAN_HUKS_TAG_LEN 16u

/*
 * 安全能力探测：确保设备级密钥存在（不存在则生成），并做一次
 * 加密/解密回环自检。返回 0 表示安全存储当前可用。
 */
int32_t sujian_huks_probe(char *error_out, uint32_t error_out_len);

/*
 * AES-GCM 加密。
 * - nonce_out：至少 SUJIAN_HUKS_NONCE_LEN 字节；由 shim 填充随机 nonce。
 * - ciphertext_out：容量至少 plaintext_len + SUJIAN_HUKS_TAG_LEN。
 * - *ciphertext_len：入口为容量，成功时写实际长度（密文 + 16 字节 tag）。
 * 允许 plaintext_len == 0（输出仅 tag），但 plaintext 为 NULL 时长度必须为 0。
 */
int32_t sujian_huks_encrypt(const uint8_t *plaintext, uint32_t plaintext_len,
                            uint8_t *nonce_out, uint32_t nonce_cap,
                            uint8_t *ciphertext_out, uint32_t *ciphertext_len,
                            char *error_out, uint32_t error_out_len);

/*
 * AES-GCM 解密。
 * - ciphertext：密文 + 16 字节 tag（tag 为最后 SUJIAN_HUKS_TAG_LEN 字节）。
 * - plaintext_out：容量至少 ciphertext_len。
 * - *plaintext_len：入口为容量，成功时写实际明文长度。
 */
int32_t sujian_huks_decrypt(const uint8_t *nonce, uint32_t nonce_len,
                            const uint8_t *ciphertext, uint32_t ciphertext_len,
                            uint8_t *plaintext_out, uint32_t *plaintext_len,
                            char *error_out, uint32_t error_out_len);

#ifdef __cplusplus
}
#endif

#endif /* SUJIAN_HUKS_STORAGE_H */
