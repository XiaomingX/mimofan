package com.jsef.benchmark.vuln;

import javax.crypto.BadPaddingException;
import javax.crypto.Cipher;
import javax.crypto.spec.IvParameterSpec;
import javax.crypto.spec.SecretKeySpec;
import java.util.Base64;

/*
 * JSEF-Benchmark L3 — Padding Oracle（CWE-327）
 *
 * 难度：L3（跨方法 / 间接：异常分支把 CBC padding 校验结果泄漏给攻击者）。
 *
 * AES/CBC/PKCS5Padding 解密。密文被篡改导致填充非法时 Cipher.doFinal 抛
 * BadPaddingException，此处上层 catch 把它映射为 HTTP 400「解密失败」，
 * 而其它错误返回 500「格式错误」。攻击者可借此区分“padding 正确/错误”，
 * 从而逐字节解密 / 伪造密文 —— 经典 padding oracle。
 *
 * 数据流：attacker ciphertext → Cipher.doFinal(CBC) → BadPaddingException
 *          分支 → 可区分的错误响应（400 vs 500）。
 *
 * CWE-327 (Use of a Broken or Risky Cryptographic Algorithm)。
 * 安全底线：仅 localhost 演示语义，不提供真实解密/伪造脚本。
 *
 * 修复要点（对照 PaddingOracleSafe.java）：改用 GCM 认证加密，解密失败
 * 统一恒时错误响应，不区分 padding 错误。
 */
public class PaddingOracleVuln {

    private static final byte[] KEY = "mysecretkey1234".getBytes();

    /**
     * 可区分响应入口：把 padding 错误映射为 400，把其它错误映射为 500。
     *
     * @param ciphertextBase64 用户可控密文
     * @return 状态码语义：400=padding 错误，500=其它错误
     */
    public int decrypt(String ciphertextBase64) {
        byte[] data = Base64.getDecoder().decode(ciphertextBase64); // 用户可控密文
        try {
            SecretKeySpec key = new SecretKeySpec(KEY, "AES");
            Cipher cipher = Cipher.getInstance("AES/CBC/PKCS5Padding");
            cipher.init(Cipher.DECRYPT_MODE, key, new IvParameterSpec(new byte[16]));
            byte[] plain = cipher.doFinal(data);          // [1] CBC 解密：篡改密文时抛 BadPaddingException
            return plain.length;                          // 正常解密路径
        } catch (BadPaddingException e) {                 // [2] 异常分支：单独捕获 padding 错误
            // [CHECKPOINT id=JSEF-PADORACLE-001 cwe=327 level=L3 source=attacker ciphertext sink=CBC decrypt with distinguishable padding error responses expect=VULN trace=benchmark/cases/vuln/padding-oracle/PaddingOracleVuln.java:44,benchmark/cases/vuln/padding-oracle/PaddingOracleVuln.java:46,benchmark/cases/vuln/padding-oracle/PaddingOracleVuln.java:48]
            return 400;                                   // [3] [VULN] 可区分响应：padding 错误 → 400（oracle 位点）
        } catch (Exception e) {
            return 500;                                   // 其它错误 → 500：与 400 可区分，构成 oracle
        }
    }

    public static void main(String[] args) {
        int status = new PaddingOracleVuln().decrypt("QmVlZmVlZmVlZg==");
        System.out.println("status=" + status);
    }
}
