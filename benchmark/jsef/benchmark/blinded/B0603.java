
package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

















@RestController
public class ReconChainSql_Source {

    private final ReconChainSql_Repo repo;

    public ReconChainSql_Source(ReconChainSql_Repo repo) {
        this.repo = repo;
    }

    @GetMapping("/benchmark/recon/sql")
    public Object list(@RequestParam String sortField) {
        /*ANCHOR_1*/
        return repo.findByDynamic(sortField); // 污点 sortField 跨文件流向 Repo
    }
}
