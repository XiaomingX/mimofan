
package blinded;

import java.util.regex.Pattern;
import java.util.regex.Matcher;









public class Redos {

    


    static boolean match(String userProvidedRegex, String userInput) {
        /*ANCHOR_1*/
        Pattern p = Pattern.compile(userProvidedRegex); // 灾难性回溯正则 (a+)+$ 由攻击者控制
        Matcher m = p.matcher(userInput);
        return m.matches();
    }

    


    static boolean matchEvil(String userInput) {
        /*ANCHOR_2*/
        Pattern p = Pattern.compile("(a+)+$"); // 灾难性回溯
        return p.matcher(userInput).matches();
    }
}
