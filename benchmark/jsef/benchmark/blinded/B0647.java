
package blinded;

import java.util.regex.Pattern;
import java.util.regex.Matcher;












public class MatcherFindUnanchoredBx {

    




    public boolean allow(String url) {
        Pattern p = Pattern.compile("https://example\\.com(/|$)");
        Matcher m = p.matcher(url);
        /*ANCHOR_1*/
        return m.find(); // find() 子串匹配：https://example.com.evil.com 通过白名单 → SSRF
    }

    public static void main(String[] args) {
        new MatcherFindUnanchoredBx().allow("https://example.com.evil.com");
    }
}
