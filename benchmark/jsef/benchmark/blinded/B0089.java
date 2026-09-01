/*
 * JSEF Benchmark 安全样本 — 弱 TLS 协议（A02，CWE-327，L2）
 * BX 版：仅启用 TLSv1.2+ 安全协议。
 * 测试点：强 SAST/LLM 应识别协议选择已安全而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
import javax.net.ssl.SSLContext;

public class WeakTlsProtocolBy {

    


    static SSLContext buildContext() throws Exception {
        /*ANCHOR_1*/
        return SSLContext.getInstance("TLSv1.2");   // 安全协议
    }
}
