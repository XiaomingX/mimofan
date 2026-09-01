/*
 * JSEF Benchmark 安全样本 — 详细错误泄露（A05，CWE-209，L2）
 * BX 版：仅返回泛化错误消息，内部细节记入服务端日志。
 * 测试点：强 SAST/LLM 应识别错误信息已泛化而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
public class VerboseErrorLeakBy {

    


    static String handle(Exception ex) {
        log(ex);   // 细节仅留服务端
        /*ANCHOR_1*/
        return "Internal error, please retry later.";   // 不泄露堆栈
    }

    static void log(Exception ex) { System.err.println(ex); }
}
