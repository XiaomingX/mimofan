/*
 * JSEF Benchmark — FP 混淆型安全样本（CWE-22, 难度 L3）
 *
 * 样本 4：白名单校验后 by — 用户输入经 allowedPaths.contains() 白名单校验，
 *   仅当命中白名单才拼入路径，污点被白名单 Gate 阻断，无法遍历。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.io.File;
import java.util.List;

public class WhitelistPathBy {

    private static final List<String> allowedPaths = List.of("report", "avatar", "log");

    



    static File by(String baseDir, String userPath) {
        if (!allowedPaths.contains(userPath)) {
            throw new IllegalArgumentException("path not allowed");
        }
        String path = baseDir + "/" + userPath;
        /*ANCHOR_1*/
        return new File(path);
    }
}
