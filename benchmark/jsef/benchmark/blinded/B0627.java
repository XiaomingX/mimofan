
package blinded;

import java.util.regex.Pattern;
import java.util.regex.Matcher;
















public class PatchRedosBx {

    



    static boolean validateByUserRegex(String userProvidedRegex, String userInput) {
        /*ANCHOR_1*/
        return userInput.matches(userProvidedRegex); // String.matches 内部 Pattern.compile 用户正则
    }
}
