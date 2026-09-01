package com.jsef.benchmark.sec;

import javax.crypto.Cipher;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;
import java.util.Base64;

/*
 * JSEF-Benchmark L3 — Padding Oracle 修复（CWE-327）
 *
 * 修复：改用 AES-GCM 认证加密。GCM 同时提供机密性与完整性校验：
 * 密文被篡改时 tag 校验失败，doFinal 抛 AEADBadTagException。
 * 解密失败统一返回同一个恒时错误响应（401），不区分“padding 错误”
 * 与其它错误，攻击者无法再观察到 padding 校验结果 —— oracle 位点被消除。
 *
 * CWE-327 (Use of a Broken or Risky Cryptographic Algorithm)。
 * 安全底线：仅 localhost 演示语义。
 */
public class PaddingOracleSafe {

    private static final byte[] KEY = "mysecretkey1234".getBytes();

    /**
     * 统一恒时错误响应：任何解密失败都返回同一状态码，不区分原因。
     *
     * @param ciphertextBase64 用户可控密文
     * @return 状态码语义：200=成功，401=解密失败（不区分原因）
     */
    public int decrypt(String ciphertextBase64) {
        byte[] data = Base64.getDecoder().decode(ciphertextBase64);
        try {
            SecretKeySpec key = new SecretKeySpec(KEY, "AES");
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, key, new GCMParameterSpec(128, new byte[12]));
            byte[] plain = cipher.doFinal(data); // GCM 认证解密：任何篡改都会被 tag 校验拒绝
            return 200;                         // 成功：返回同一成功响应
        } catch (Exception e) {
            // [CHECKPOINT id=JSEF-PADORACLE-001S cwe=327 level=L3 source=attacker ciphertext sink=GCM authenticated decrypt + uniform constant-time error response expect=SAFE]
            return 401;                         // 统一恒时错误响应：不区分 padding/tag/其它错误
        }
    }

    public static void main(String[] args) {
        int status = new PaddingOracleSafe().decrypt("QmVlZmVlZmVlZg==");
        System.out.println("status=" + status);
    }
}
