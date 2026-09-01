
package blinded;

import java.io.IOException;









public class DistractorForkCmd_By {

    private static final String[] ALLOWED = {"echo", "ls", "date"};

    public String handle(String cmd) throws IOException {
        /*ANCHOR_1*/
        boolean ok = false;
        for (String a : ALLOWED) {
            if (cmd.startsWith(a)) { ok = true; break; }
        }
        if (!ok) {
            return "blocked"; // 不可信命令被拒，无法到达 sink
        }
        Process p = Runtime.getRuntime().exec(new String[]{"sh", "-c", cmd});
        return "pid=" + p.pid();
    }
}
