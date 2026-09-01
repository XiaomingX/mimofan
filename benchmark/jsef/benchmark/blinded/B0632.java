package blinded;

import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;






















public class RegexCompileInjection {

    


    public boolean compileAndMatch(String userRegex, String input) {
        /*ANCHOR_1*/
        // 缺陷：外部可控正则被直接编译，恶意 pattern 在特定输入上触发灾难性回溯 → CPU DoS
        Pattern p = Pattern.compile(userRegex);
        return p.matcher(input).matches();
    }

    public static void main(String[] args) {
        try {
            boolean ok = new RegexCompileInjection().compileAndMatch("(a+)+$", "aaaa");
            System.out.println(ok);
        } catch (PatternSyntaxException e) {
            System.out.println("bad regex");
        }
    }
}
