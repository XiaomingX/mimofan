
package blinded;

import java.util.regex.Pattern;
import java.util.regex.Matcher;







public class RedosBy {

    // 预编译的安全正则：原子、无嵌套量词，线性时间
    private static final Pattern BX_PATTERN = Pattern.compile("[a-z]+");

    


    static boolean match(String userInput) {
        if (userInput.length() > 1000) {
            return false; // 限制输入长度，进一步降低 DoS 风险
        }
        /*ANCHOR_1*/
        Matcher m = BX_PATTERN.matcher(userInput);
        return m.matches();
    }
}
