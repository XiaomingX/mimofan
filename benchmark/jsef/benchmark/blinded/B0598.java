
package blinded;

import java.io.IOException;




















public class DistractorForkCmd {

    private final ServiceB serviceB;

    public DistractorForkCmd(ServiceB serviceB) {
        this.serviceB = serviceB;
    }

    
    public void auditLog(String cmd) {
        System.out.println("[audit] " + cmd); // 无害：仅日志
    }

    
    public String filteredExec(String cmd) {
        if (!cmd.startsWith("allowed:")) {
            return "blocked"; // 白名单拦截，假 sink
        }
        return runReal(cmd);
    }

    public String handle(String cmd) throws IOException {
        auditLog(cmd);          // 无害分叉（干扰）
        filteredExec(cmd);      // 假 sink（干扰）
        /*ANCHOR_1*/
        return serviceB.execute(cmd); // 真 sink：污点直达 Runtime.exec
    }

    private String runReal(String cmd) throws IOException {
        Process p = Runtime.getRuntime().exec(cmd);
        return "pid=" + p.pid();
    }

    
    public static class ServiceB {
        public String execute(String data) throws IOException {
            Process p = Runtime.getRuntime().exec(data);
            return "pid=" + p.pid();
        }
    }
}
