package blinded;

import java.util.regex.Pattern;






















public class RegexCompileInjection_By {

    // 固定、锚定的白名单正则：仅允许简单字母数字标识，无嵌套量词
    private static final Pattern BX_PATTERN = Pattern.compile("^[A-Za-z0-9_]+$");

    


    public boolean byMatch(String input) {
        /*ANCHOR_1*/
        // 修复：pattern 为固定白名单常量，不可信输入仅作为 matcher 内容，复杂度可控，无 ReDoS
        return BX_PATTERN.matcher(input).matches();
    }

    public static void main(String[] args) {
        System.out.println(new RegexCompileInjection_By().byMatch("user_123"));
    }
}
