
package blinded;

import org.springframework.web.bind.annotation.ModelAttribute;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RestController;










@RestController
public class Spring4ShellBinding {

    public static class Account {
        private String name;
        public String getName() { return name; }
        public void setName(String name) { this.name = name; }
    }

    




    @PostMapping("/account/update")
    public String update(@ModelAttribute Account account) { // 未限定可写字段
        /*ANCHOR_1*/
        return "updated " + account.getName();
    }
}
