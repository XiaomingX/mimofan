
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;






@RestController
public class CsrfTransferBy {

    private static final String ORIGIN = "https://bank.example.com";

    


    @PostMapping("/api/transfer")
    public String transfer(@RequestParam String to, @RequestParam double amount,
                           @RequestHeader("Origin") String origin) {
        /*ANCHOR_1*/
        if (!ORIGIN.equals(origin)) {
            return "forbidden: cross-site request";
        }
        return "transferred " + amount + " to " + to;
    }
}
