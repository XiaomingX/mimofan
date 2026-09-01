
package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;









@RestController
public class ReconChainSql_By {

    private static final java.util.Set<String> ALLOWED = java.util.Set.of("id", "name", "createdAt");

    @GetMapping("/benchmark/recon/sql/by")
    public Object list(@RequestParam String sortField) {
        /*ANCHOR_1*/
        if (!ALLOWED.contains(sortField)) {
            return "invalid sort field"; // 不可信字段被拒，无法到达查询
        }
        String methodName = "findBy" + sortField; // 白名单内字段才可拼入
        System.out.println("[abstract sql] select * from t order by " + methodName);
        return "rows";
    }
}
