package blinded;












public class BuildScriptInjection {

    


    public static void runBuild(String branchName) throws Exception {
        // source：不可信的 branch/pr 参数
        /*ANCHOR_1*/
        Runtime.getRuntime().exec("git checkout " + branchName + " && ./build.sh");
    }
}
