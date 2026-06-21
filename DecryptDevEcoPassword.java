import java.security.MessageDigest;

import java.io.*;
import java.lang.reflect.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import javax.crypto.*;
import javax.crypto.spec.*;

public class DecryptDevEcoPassword {
    
    public static void main(String[] args) throws Exception {
        // 读取build-profile.json5中的加密密码
        String storePasswordEncrypted = "0000001B20D1A538BCEA2F2934125CB51FE66D2362001B469CC80F5657698897091F83AE78002E6367AB1D";
        String keyPasswordEncrypted = "0000001B0D7991BA59449BD34156D20B72291FA296C6107B7C630C51FC7AC618854707746E1BE3044CD2C9";
        
        // IntelliJ PasswordSafe加密格式:
        // 前8个hex字符是版本标识(0000001B)
        // 后面是hex编码的加密数据
        
        // 尝试直接解密
        System.out.println("Store password encrypted: " + storePasswordEncrypted);
        System.out.println("Key password encrypted: " + keyPasswordEncrypted);
        
        // 解析加密数据
        byte[] storeEncryptedBytes = hexToBytes(storePasswordEncrypted.substring(8));
        byte[] keyEncryptedBytes = hexToBytes(keyPasswordEncrypted.substring(8));
        
        System.out.println("Store encrypted bytes length: " + storeEncryptedBytes.length);
        System.out.println("Key encrypted bytes length: " + keyEncryptedBytes.length);
        
        // 尝试使用不同的密钥派生方式
        // IntelliJ的EncryptionUtil使用SHA256从serviceName+userName派生密钥
        // 对于DevEco的签名配置，serviceName可能是:
        // "com.huawei.deveco.projectmgmt.hos.signature.sign"
        // userName可能是: "default" 或 "storePassword" 或 "keyPassword"
        
        String[] serviceNames = {
            "com.huawei.deveco.projectmgmt.hos.signature.sign",
            "com.huawei.deveco.projectmgmt.ohos.signature",
            "HarmonyOS Signing",
            "OhosSigningConfig",
            "default",
            "com.huawei.deveco.projectmodel.ohos.model.impl.OhosSigningConfig",
            "com.huawei.deveco.projectmgmt.ohos.signature.common.SignConfigManager",
        };
        
        String[] userNames = {
            "storePassword",
            "keyPassword",
            "default",
            "debugKey",
            "",
        };
        
        for (String serviceName : serviceNames) {
            for (String userName : userNames) {
                try {
                    byte[] rawKey = rawKey(serviceName, userName);
                    byte[] decrypted = decryptData(rawKey, storeEncryptedBytes);
                    if (decrypted != null) {
                        String password = new String(decrypted, StandardCharsets.UTF_8);
                        System.out.println("SUCCESS! ServiceName: " + serviceName + ", UserName: " + userName);
                        System.out.println("Store Password: " + password);
                        
                        // 也解密keyPassword
                        byte[] keyDecrypted = decryptData(rawKey, keyEncryptedBytes);
                        if (keyDecrypted != null) {
                            String keyPwd = new String(keyDecrypted, StandardCharsets.UTF_8);
                            System.out.println("Key Password: " + keyPwd);
                        }
                        return;
                    }
                } catch (Exception e) {
                    // 忽略解密失败
                }
            }
        }
        
        System.out.println("Failed to decrypt with all combinations");
    }
    
    // 模拟IntelliJ的rawKey生成
    // EncryptionUtil.rawKey() 使用SHA256(serviceName + userName)
    static byte[] rawKey(String serviceName, String userName) {
        try {
            String key = serviceName + userName;
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            byte[] hash = digest.digest(key.getBytes(StandardCharsets.UTF_8));
            // 取前16字节作为AES密钥
            return Arrays.copyOf(hash, 16);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }
    
    // 模拟IntelliJ的decryptData
    // EncryptionUtil.decryptData() 使用AES/CBC/PKCS5Padding
    static byte[] decryptData(byte[] keyBytes, byte[] encryptedData) {
        try {
            // 前16字节是IV
            if (encryptedData.length < 16) return null;
            byte[] iv = Arrays.copyOf(encryptedData, 16);
            byte[] ciphertext = Arrays.copyOfRange(encryptedData, 16, encryptedData.length);
            
            SecretKeySpec keySpec = new SecretKeySpec(keyBytes, "AES");
            IvParameterSpec ivSpec = new IvParameterSpec(iv);
            
            Cipher cipher = Cipher.getInstance("AES/CBC/PKCS5Padding");
            cipher.init(Cipher.DECRYPT_MODE, keySpec, ivSpec);
            
            return cipher.doFinal(ciphertext);
        } catch (Exception e) {
            return null;
        }
    }
    
    static byte[] hexToBytes(String hex) {
        byte[] bytes = new byte[hex.length() / 2];
        for (int i = 0; i < hex.length(); i += 2) {
            bytes[i / 2] = (byte) Integer.parseInt(hex.substring(i, i + 2), 16);
        }
        return bytes;
    }
}