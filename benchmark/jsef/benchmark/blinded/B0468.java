/*
 * JSEF Benchmark 样本 — 弱 TLS 协议（A02，CWE-327，L2）
 * 运行态需 JSEF 依赖（javax.net.ssl）；独立 benchmark 源文件，不强求编译。
 * 安全底线：仅 localhost 演示语义，不写真实中间人利用。
 *
 * 知识点（A02 加密缺陷，CWE-327 危险加密算法）：
 *   SSLContext 启用已废弃且不安全的 TLSv1.0 协议，易受降级/中间人攻击。
 *   污点：无显式不可信输入，但协议选择本身是危险终点（sink 为协议启用）。
 */
import javax.net.ssl.SSLContext;

public class WeakTlsProtocol {

    


    static SSLContext buildContext() throws Exception {
        /*ANCHOR_1*/
        return SSLContext.getInstance("TLSv1.0");   // 弱协议：可被降级攻击
    }
}
