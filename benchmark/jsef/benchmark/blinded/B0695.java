/*
 * JSEF Benchmark — FP 混淆型安全样本的对照 bx（CWE-78, 难度 L3）
 *
 * 对照样本：命令字符串由不可信用户输入拼接，可注入额外命令。
 * 与 ConstantCommandBy 的纯常量命令形成 bx/by 对。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.io.IOException;

public class UserInputCommandInjection {

    



    static void unby(String userInput) throws IOException {
        String cmd = "ping -c 1 " + userInput;
        /*ANCHOR_1*/
        Runtime.getRuntime().exec(cmd);
    }
}
