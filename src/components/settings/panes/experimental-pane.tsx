import { useTranslation } from "react-i18next";
import {
	PageTitle,
	SettingsGroup,
	SettingsRow,
} from "@/components/settings/settings-layout";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { DEFAULT_JEV_BASE_URL } from "@/lib/settings/defaults";
import type { AppSettings } from "@/lib/settings/types";

export function ExperimentalPane({
	settings,
	patch,
}: {
	settings: AppSettings;
	patch: (p: Partial<AppSettings>) => void;
}) {
	const { t } = useTranslation("settings");
	const jev = settings.jev;

	const updateJev = (partial: Partial<AppSettings["jev"]>) => {
		patch({ jev: { ...jev, ...partial } });
	};

	return (
		<div className="space-y-6">
			<PageTitle title={t("experimental.title")} />

			<SettingsGroup>
				<SettingsRow
					label={t("experimental.jev.section")}
					description={t("experimental.jev.description")}
				>
					<div className="flex w-full min-w-0 flex-col gap-3">
						<div className="flex items-center gap-2">
							<Label
								htmlFor="jev-api-key"
								className="w-20 shrink-0 font-normal text-muted-foreground text-xs"
							>
								{t("experimental.jev.apiKey.label")}
							</Label>
							<Input
								id="jev-api-key"
								type="password"
								value={jev.apiKey}
								placeholder={t("experimental.jev.apiKey.placeholder")}
								className="h-8 min-w-0 flex-1 font-mono text-xs placeholder:text-muted-foreground/50"
								spellCheck={false}
								autoComplete="off"
								onChange={(e) => updateJev({ apiKey: e.target.value })}
								onFocus={(e) => e.target.select()}
							/>
						</div>
						<div className="flex items-center gap-2">
							<Label
								htmlFor="jev-base-url"
								className="w-20 shrink-0 font-normal text-muted-foreground text-xs"
							>
								{t("experimental.jev.baseUrl.label")}
							</Label>
							<Input
								id="jev-base-url"
								type="text"
								value={jev.baseUrl}
								placeholder={DEFAULT_JEV_BASE_URL}
								className="h-8 min-w-0 flex-1 font-mono text-xs placeholder:text-muted-foreground/50"
								spellCheck={false}
								autoComplete="off"
								onChange={(e) => updateJev({ baseUrl: e.target.value })}
							/>
						</div>
					</div>
				</SettingsRow>
			</SettingsGroup>
		</div>
	);
}
