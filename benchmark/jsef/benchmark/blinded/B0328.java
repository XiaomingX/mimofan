/*
 * JSEF Benchmark 样本 — 隐式信任内部输入：边界校验后再处理（by 对照，CWE-94，L3）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class TrustInternalInputBy {

    static final class InternalResp { final String payload; InternalResp(String p){ this.payload=p; } }

    // 安全：跨信任边界的数据先经 schema/范围校验，再保守处理
    static Object handle(InternalResp resp) {
        /*ANCHOR_1*/
        if (resp.payload == null || !resp.payload.matches("[a-zA-Z0-9 ]+")) {
            throw new SecurityException("internal payload failed boundary validation");
        }
        return "handled:" + resp.payload;   // 不再 eval，仅做白名单处理
    }
}
