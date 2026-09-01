/*
 * JSEF Benchmark — FP 混淆型安全样本的对照 bx（CWE-22, 难度 L3）
 *
 * 对照样本：用户输入未校验直接拼入路径，可路径遍历（"../../etc/passwd"）。
 * 与 WhitelistPathBy 的白名单 Gate 形成 bx/by 对。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.io.File;

public class PathTraversalInjection {

    



    static File unby(String baseDir, String userPath) {
        String path = baseDir + "/" + userPath;
        /*ANCHOR_1*/
        return new File(path);
    }
}
