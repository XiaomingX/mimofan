package blinded;

import java.util.Arrays;
import java.util.List;






public class BuildScriptInjectionBy {

    private static final List<String> ALLOWED = Arrays.asList("main", "release", "develop");

    


    public static void runBuild(String branchName) throws Exception {
        // source：不可信的 branch/pr 参数
        if (!ALLOWED.contains(branchName)) {
            throw new IllegalArgumentException("非法分支名: " + branchName);
        }
        /*ANCHOR_1*/
        new ProcessBuilder("git", "checkout", branchName).inheritIO().start();
    }
}
