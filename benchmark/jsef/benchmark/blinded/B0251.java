package blinded;

import java.util.regex.Pattern;
import java.util.regex.Matcher;










public class MatcherFindUnanchoredBy {

    




    public boolean allow(String url) {
        Pattern p = Pattern.compile("https://example\\.com(/|$)");
        Matcher m = p.matcher(url);
        /*ANCHOR_1*/
        return m.matches(); // matches() 全串锚定：https://example.com.evil.com 被拒
    }

    public static void main(String[] args) {
        new MatcherFindUnanchoredBy().allow("https://example.com.evil.com");
    }
}
