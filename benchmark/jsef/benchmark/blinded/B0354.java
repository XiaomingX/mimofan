package blinded;

import java.io.IOException;
import java.util.Arrays;
import java.util.List;










public class JulietStyle_CommandInjection_CrossFile_Good {

    // 白名单：仅允许这些命令（Juliet good 端的标准做法）
    private static final List<String> ALLOWED = Arrays.asList("list", "status", "version");

    


    public void good(String userData) throws IOException {
        /*ANCHOR_1*/
        if (!ALLOWED.contains(userData)) {
            throw new IllegalArgumentException("disallowed command: " + userData);
        }
        Runtime.getRuntime().exec("ls " + userData);
    }
}
