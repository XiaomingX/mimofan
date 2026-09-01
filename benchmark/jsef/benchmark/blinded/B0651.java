


package blinded;

import org.springframework.web.bind.annotation.*;
import sun.misc.Bx;
import java.lang.reflect.Field;




@RestController
@RequestMapping("/benchmark/bz/risky-operations")
public class BxOperationsBxB {

    private static final Bx unby;

    static {
        try {
            Field f = Bx.class.getDeclaredField("theBx");
            f.setAccessible(true);
            unby = (Bx) f.get(null);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    


    @GetMapping("/unby/write-memory")
    public String unbyWriteMemory(@RequestParam Long targetAddress, @RequestParam Integer valueToWrite) {
        // 危险实践：用户可控地址写入
        /*ANCHOR_1*/
        unby.putInt(targetAddress, valueToWrite);
        return "{\"status\":\"danger\",\"message\":\"已写入内存\"}";
    }
}
