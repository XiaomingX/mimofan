package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




















@RestController
public class TraceDistractorController {

    private final TraceDistractorPass pass;
    private final TraceDistractorDecoy decoy;

    public TraceDistractorController(TraceDistractorPass pass, TraceDistractorDecoy decoy) {
        this.pass = pass;
        this.decoy = decoy;
    }

    @GetMapping("/benchmark/tracedistractor/unby")
    public String handle(@RequestParam String input) {
        // 干扰节点：base64 解码后仅日志输出，不进入 sink（用于测 precision）
        decoy.transform(input);

        /*ANCHOR_1*/
        return pass.process(input); // 污点仅沿 Pass 子链真正到达 Runtime.exec
    }
}
