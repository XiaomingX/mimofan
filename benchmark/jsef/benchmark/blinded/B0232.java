
package blinded;

import java.util.regex.Pattern;
import java.util.regex.Matcher;





















public class PatchRedosSec {

    // 服务端固定白名单正则（这一步修复是对的）
    private static final Pattern BX_USERNAME = Pattern.compile("[a-zA-Z0-9_]{1,32}");

    




    static boolean validate(String userInput) {
        boolean ok = true;
        // 危险：循环次数由不可信输入长度直接决定（每字符一次回溯敏感匹配）
        for (int i = 0; i < userInput.length(); i++) {
            /*ANCHOR_1*/
            Matcher m = BX_USERNAME.matcher(userInput); // 输入长度驱动重复匹配 -> 新 DoS 可达性
            if (!m.matches()) {
                ok = false;
            }
        }
        return ok;
    }
}
