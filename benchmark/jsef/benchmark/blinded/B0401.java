package blinded;












public class TraceDistractorPass {

    


    public String process(String input) {
        String cmd = input; // 污点直接透传（本行是 trace 真节点）
        return run(cmd);    // 语义等价：Runtime.exec(cmd)
    }


    private String run(String cmd) {
        System.out.println("[cmd-exec] " + cmd); // 语义等价: Runtime.exec(cmd)
        return "ran: " + cmd;
    }
}
