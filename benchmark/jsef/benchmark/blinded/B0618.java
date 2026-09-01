/*
 * JSEF Benchmark 样本 — 来源/签名/完整性校验缺失：签名 token 未校验即信任（BxGym 子类 BL-ORIGIN-INTEGRITY，CWE-347，L3）
 * 独立 benchmark 源文件，不强求编译。安全底线：仅 localhost 演示语义，不写真实利用脚本。
 *
 * 知识点：漏洞核心在"完整性语义"——系统解析携带签名的 token 后直接信任其中的声明（如 role），
 * 未校验签名有效性。数据流干净，但缺失签名校验。静态分析需在 claims.get("role") 处识别"签名未验"。
 */
package blinded;

public class SignedTokenNoVerify {

    // 演示用：token 解析结果
    static final class Claims { final String role; Claims(String role){ this.role = role; } }

    // 危险：解析 token 但不校验签名，直接信任声明
    static Claims parse(String token) {
        // source：不可信 token（HTTP 参数，攻击者可控）
        /*ANCHOR_1*/
        Claims claims = new Claims(token.split("\\.")[0]);   // 伪造 token 即可注入 role
        return claims;
    }
}
