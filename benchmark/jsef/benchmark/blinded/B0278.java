
package blinded;

import org.springframework.web.bind.WebDataBinder;
import org.springframework.web.bind.annotation.InitBinder;
import org.springframework.web.bind.annotation.ModelAttribute;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RestController;







@RestController
public class Spring4ShellBindingBy {

    public static class AccountDto {
        private String name;
        public String getName() { return name; }
        public void setName(String name) { this.name = name; }
    }

    @InitBinder
    public void initBinder(WebDataBinder binder) {
        /*ANCHOR_1*/
        binder.setDisallowedFields("class.*", "module.*", "*.class.*", "*.module.*");
    }

    @PostMapping("/account/update")
    public String update(@ModelAttribute AccountDto account) {
        return "updated " + account.getName();
    }
}
