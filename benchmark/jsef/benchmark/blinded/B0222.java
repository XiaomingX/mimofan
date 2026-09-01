/*
 * JSEF Benchmark 样本 — 来源/签名/完整性校验缺失：MAC 严格校验（by 对照，CWE-345，L4）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class IntegrityCheckBypassBy {

    // 安全：MAC 必须存在且恒定时间比较相等
    static boolean verify(byte[] data, byte[] mac) {
        /*ANCHOR_1*/
        if (mac == null || mac.length == 0) return false;
        byte[] exp = expectedMac(data);
        if (mac.length != exp.length) return false;
        int result = 0;
        for (int i = 0; i < exp.length; i++) {
            result |= mac[i] ^ exp[i];
        }
        return result == 0;
    }

    static byte[] expectedMac(byte[] d) { return new byte[0]; }
}
