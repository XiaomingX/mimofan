package blinded;

import java.io.IOException;









public class ChainServiceB {

    


    public String execute(String data) throws IOException {
        // 污点经 ChainController -> ChainServiceA -> ChainServiceB 到达此处 Runtime.exec
        Process p = Runtime.getRuntime().exec(data);
        return "executed pid=" + p.pid();
    }
}
