import { Subcomponent } from "@anvil-di/anvil";
import { Handler } from "./handler";

@Subcomponent({ modules: [] })
export abstract class RequestComponent {
  abstract handler(): Handler;
}
