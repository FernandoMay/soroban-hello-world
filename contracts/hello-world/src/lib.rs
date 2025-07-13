#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Env, String, Address, BytesN, Bytes, Vec};

// Savia Smart Contracts for Stellar
// Fixed version compatible with Soroban

// ========== DATA STRUCTURES ==========

#[derive(Clone)]
#[contracttype]
pub struct Campaign {
    pub id: BytesN<32>,
    pub title: String,
    pub description: String,
    pub beneficiary: Address,
    pub goal_amount: u64,
    pub current_amount: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub verified: bool,
    pub trust_score: u32,
    pub category: String,
    pub location: String,
}

#[derive(Clone)]
#[contracttype]
pub struct Donation {
    pub id: BytesN<32>,
    pub campaign_id: BytesN<32>,
    pub donor: Address,
    pub amount: u64,
    pub timestamp: u64,
    pub nft_minted: bool,
    pub anonymous: bool,
}

#[derive(Clone)]
#[contracttype]
pub struct TrustScore {
    pub entity: Address,
    pub score: u32,
    pub verification_level: u32,
    pub donation_count: u32,
    pub total_donated: u64,
    pub campaigns_created: u32,
    pub last_updated: u64,
}

#[derive(Clone)]
#[contracttype]
pub struct NFTBadge {
    pub id: BytesN<32>,
    pub owner: Address,
    pub badge_type: String,
    pub campaign_id: Option<BytesN<32>>,
    pub minted_at: u64,
    pub metadata_uri: String,
}

#[derive(Clone)]
#[contracttype]
pub struct Disbursement {
    pub id: BytesN<32>,
    pub campaign_id: BytesN<32>,
    pub recipient: Address,
    pub amount: u64,
    pub milestone: String,
    pub status: DisbursementStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
}

#[derive(Clone, PartialEq)]
#[contracttype]
pub enum DisbursementStatus {
    Pending,
    Approved,
    Executed,
    Rejected,
}

// ========== STORAGE KEYS ==========

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Campaign(BytesN<32>),
    Donation(BytesN<32>),
    TrustScore(Address),
    NFTBadge(BytesN<32>),
    Disbursement(BytesN<32>),
    PlatformFee,
    CampaignCounter,
    DonationCounter,
    NFTCounter,
    DisbursementCounter,
}

// ========== ERROR CODES ==========

#[derive(Clone, Copy, Debug, PartialEq)]
#[contracttype]
pub enum SaviaError {
    InvalidFee = 1,
    InvalidGoal = 2,
    InvalidDuration = 3,
    CampaignNotFound = 4,
    CampaignEnded = 5,
    InvalidAmount = 6,
    ScoreExists = 7,
    InsufficientFunds = 8,
    DisbursementNotFound = 9,
    NotApproved = 10,
}

// ========== MAIN CONTRACT ==========

#[contract]
pub struct SaviaContract;

#[contractimpl]
impl SaviaContract {
    
    /// Initialize the contract with platform fee
    pub fn initialize(env: Env, platform_fee: u64) -> Result<(), SaviaError> {
        if platform_fee > 1000 {
            return Err(SaviaError::InvalidFee);
        }
        
        env.storage().instance().set(&DataKey::PlatformFee, &platform_fee);
        env.storage().instance().set(&DataKey::CampaignCounter, &0u64);
        env.storage().instance().set(&DataKey::DonationCounter, &0u64);
        env.storage().instance().set(&DataKey::NFTCounter, &0u64);
        env.storage().instance().set(&DataKey::DisbursementCounter, &0u64);
        
        Ok(())
    }

    /// Create a new campaign
    pub fn create_campaign(
        env: Env,
        beneficiary: Address,
        title: String,
        description: String,
        goal_amount: u64,
        duration_days: u64,
        category: String,
        location: String,
    ) -> Result<BytesN<32>, SaviaError> {
        // Validate inputs
        if goal_amount == 0 {
            return Err(SaviaError::InvalidGoal);
        }
        
        if duration_days == 0 || duration_days > 365 {
            return Err(SaviaError::InvalidDuration);
        }

        // Get and increment campaign counter
        let counter: u64 = env.storage().instance().get(&DataKey::CampaignCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::CampaignCounter, &new_counter);

        // Generate campaign ID using existing data
        let current_time = env.ledger().timestamp();
        let mut hash_input = Bytes::new(&env);
        
        // Convert Address to bytes properly - use Vec<u8> approach
        let beneficiary_str = beneficiary.to_string();
        let beneficiary_bytes = Bytes::from_slice(&env, beneficiary_str.as_str().as_bytes());
        let title_bytes = Bytes::from_slice(&env, title.to_string().as_str().as_bytes());
        
        hash_input.append(&beneficiary_bytes);
        hash_input.append(&title_bytes);
        hash_input.append(&Bytes::from_slice(&env, &goal_amount.to_be_bytes()));
        hash_input.append(&Bytes::from_slice(&env, &current_time.to_be_bytes()));
        hash_input.append(&Bytes::from_slice(&env, &new_counter.to_be_bytes()));
        
        let campaign_id = env.crypto().sha256(&hash_input);

        let end_time = current_time + (duration_days * 24 * 60 * 60); // Convert to seconds

        let campaign = Campaign {
            id: campaign_id,
            title,
            description,
            beneficiary,
            goal_amount,
            current_amount: 0,
            start_time: current_time,
            end_time,
            verified: false,
            trust_score: 0,
            category,
            location,
        };

        env.storage().persistent().set(&DataKey::Campaign(campaign_id), &campaign);
        
        Ok(campaign_id)
    }

    /// Get campaign details
    pub fn get_campaign(env: Env, campaign_id: BytesN<32>) -> Option<Campaign> {
        env.storage().persistent().get(&DataKey::Campaign(campaign_id))
    }

    /// Verify a campaign (admin function)
    pub fn verify_campaign(
        env: Env,
        campaign_id: BytesN<32>,
        trust_score: u32,
    ) -> Result<(), SaviaError> {
        let mut campaign: Campaign = env.storage().persistent().get(&DataKey::Campaign(campaign_id))
            .ok_or(SaviaError::CampaignNotFound)?;

        campaign.verified = true;
        campaign.trust_score = trust_score;

        env.storage().persistent().set(&DataKey::Campaign(campaign_id), &campaign);
        Ok(())
    }

    /// Process a donation
    pub fn donate(
        env: Env,
        campaign_id: BytesN<32>,
        donor: Address,
        amount: u64,
        anonymous: bool,
        mint_nft: bool,
    ) -> Result<BytesN<32>, SaviaError> {
        // Validate campaign exists and is active
        let mut campaign: Campaign = env.storage().persistent().get(&DataKey::Campaign(campaign_id))
            .ok_or(SaviaError::CampaignNotFound)?;

        let current_time = env.ledger().timestamp();
        if current_time > campaign.end_time {
            return Err(SaviaError::CampaignEnded);
        }

        if amount == 0 {
            return Err(SaviaError::InvalidAmount);
        }

        // Get platform fee
        let platform_fee_rate: u64 = env.storage().instance().get(&DataKey::PlatformFee).unwrap_or(200);
        let platform_fee = (amount * platform_fee_rate) / 10000;
        let net_amount = amount - platform_fee;

        // Get and increment donation counter
        let counter: u64 = env.storage().instance().get(&DataKey::DonationCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::DonationCounter, &new_counter);

        // Generate donation ID
        let mut hash_input = Bytes::new(&env);
        
        // Convert to bytes properly
        let campaign_bytes = Bytes::from_slice(&env, campaign_id.to_array().as_slice());
        let donor_str = donor.to_string();
        let donor_bytes = Bytes::from_slice(&env, donor_str.as_str().as_bytes());
        
        hash_input.append(&campaign_bytes);
        hash_input.append(&donor_bytes);
        hash_input.append(&Bytes::from_slice(&env, &amount.to_be_bytes()));
        hash_input.append(&Bytes::from_slice(&env, &current_time.to_be_bytes()));
        hash_input.append(&Bytes::from_slice(&env, &new_counter.to_be_bytes()));
        
        let donation_id = env.crypto().sha256(&hash_input);

        // Create donation record
        let donation = Donation {
            id: donation_id,
            campaign_id,
            donor: donor.clone(),
            amount: net_amount,
            timestamp: current_time,
            nft_minted: mint_nft,
            anonymous,
        };

        // Update campaign progress
        campaign.current_amount += net_amount;
        env.storage().persistent().set(&DataKey::Campaign(campaign_id), &campaign);

        // Store donation
        env.storage().persistent().set(&DataKey::Donation(donation_id), &donation);

        // Update trust score
        Self::update_donor_trust_score(env.clone(), donor.clone(), net_amount)?;

        // Mint NFT if requested
        if mint_nft {
            Self::mint_donation_nft(env.clone(), donor, campaign_id, donation_id, net_amount)?;
        }

        Ok(donation_id)
    }

    /// Get donation details
    pub fn get_donation(env: Env, donation_id: BytesN<32>) -> Option<Donation> {
        env.storage().persistent().get(&DataKey::Donation(donation_id))
    }

    /// Initialize trust score for new user
    pub fn initialize_trust_score(env: Env, entity: Address) -> Result<(), SaviaError> {
        if env.storage().persistent().has(&DataKey::TrustScore(entity.clone())) {
            return Err(SaviaError::ScoreExists);
        }

        let trust_score = TrustScore {
            entity: entity.clone(),
            score: 50, // Start with neutral score
            verification_level: 0,
            donation_count: 0,
            total_donated: 0,
            campaigns_created: 0,
            last_updated: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::TrustScore(entity), &trust_score);
        Ok(())
    }

    /// Update donor trust score
    fn update_donor_trust_score(env: Env, donor: Address, amount: u64) -> Result<(), SaviaError> {
        let mut trust_score: TrustScore = env.storage().persistent().get(&DataKey::TrustScore(donor.clone()))
            .unwrap_or(TrustScore {
                entity: donor.clone(),
                score: 50,
                verification_level: 0,
                donation_count: 0,
                total_donated: 0,
                campaigns_created: 0,
                last_updated: env.ledger().timestamp(),
            });

        trust_score.donation_count += 1;
        trust_score.total_donated += amount;
        trust_score.last_updated = env.ledger().timestamp();

        // Calculate new score based on donation history
        let donation_factor = if trust_score.donation_count > 100 { 100 } else { trust_score.donation_count };
        let amount_factor = if trust_score.total_donated > 100000 { 100000 } else { trust_score.total_donated };
        let consistency_factor = if trust_score.donation_count > 1 { 120u64 } else { 100u64 };

        // Fixed arithmetic types
        let new_score = 50u64 + (25u64 * donation_factor as u64 / 100u64) + (20u64 * amount_factor / 100000u64) * consistency_factor / 100u64;
        trust_score.score = if new_score > 100 { 100 } else { new_score as u32 };

        env.storage().persistent().set(&DataKey::TrustScore(donor), &trust_score);
        Ok(())
    }

    /// Get trust score
    pub fn get_trust_score(env: Env, entity: Address) -> Option<TrustScore> {
        env.storage().persistent().get(&DataKey::TrustScore(entity))
    }

    /// Mint donation NFT
    fn mint_donation_nft(
        env: Env,
        owner: Address,
        campaign_id: BytesN<32>,
        donation_id: BytesN<32>,
        amount: u64,
    ) -> Result<BytesN<32>, SaviaError> {
        // Get and increment NFT counter
        let counter: u64 = env.storage().instance().get(&DataKey::NFTCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::NFTCounter, &new_counter);

        // Generate NFT ID
        let mut hash_input = Bytes::new(&env);
        
        let owner_str = owner.to_string();
        let owner_bytes = Bytes::from_slice(&env, owner_str.as_str().as_bytes());
        let campaign_bytes = Bytes::from_slice(&env, campaign_id.to_array().as_slice());
        let donation_bytes = Bytes::from_slice(&env, donation_id.to_array().as_slice());
        
        hash_input.append(&owner_bytes);
        hash_input.append(&campaign_bytes);
        hash_input.append(&donation_bytes);
        hash_input.append(&Bytes::from_slice(&env, &amount.to_be_bytes()));
        hash_input.append(&Bytes::from_slice(&env, &new_counter.to_be_bytes()));
        
        let nft_id = env.crypto().sha256(&hash_input);

        // Determine badge type based on amount
        let badge_type = Self::get_badge_type(&env, amount);

        let nft_badge = NFTBadge {
            id: nft_id,
            owner,
            badge_type,
            campaign_id: Some(campaign_id),
            minted_at: env.ledger().timestamp(),
            metadata_uri: String::from_str(&env, "https://savia.org/nft/metadata"),
        };

        env.storage().persistent().set(&DataKey::NFTBadge(nft_id), &nft_badge);
        Ok(nft_id)
    }

    /// Get NFT details
    pub fn get_nft(env: Env, nft_id: BytesN<32>) -> Option<NFTBadge> {
        env.storage().persistent().get(&DataKey::NFTBadge(nft_id))
    }

    /// Create disbursement request
    pub fn create_disbursement(
        env: Env,
        campaign_id: BytesN<32>,
        recipient: Address,
        amount: u64,
        milestone: String,
    ) -> Result<BytesN<32>, SaviaError> {
        let campaign: Campaign = env.storage().persistent().get(&DataKey::Campaign(campaign_id))
            .ok_or(SaviaError::CampaignNotFound)?;

        if amount > campaign.current_amount {
            return Err(SaviaError::InsufficientFunds);
        }

        // Get and increment disbursement counter
        let counter: u64 = env.storage().instance().get(&DataKey::DisbursementCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::DisbursementCounter, &new_counter);

        // Generate disbursement ID
        let mut hash_input = Bytes::new(&env);
        
        let campaign_bytes = Bytes::from_slice(&env, campaign_id.to_array().as_slice());
        let recipient_str = recipient.to_string();
        let recipient_bytes = Bytes::from_slice(&env, recipient_str.as_str().as_bytes());
        let milestone_bytes = Bytes::from_slice(&env, milestone.to_string().as_str().as_bytes());
        
        hash_input.append(&campaign_bytes);
        hash_input.append(&recipient_bytes);
        hash_input.append(&Bytes::from_slice(&env, &amount.to_be_bytes()));
        hash_input.append(&milestone_bytes);
        hash_input.append(&Bytes::from_slice(&env, &new_counter.to_be_bytes()));
        
        let disbursement_id = env.crypto().sha256(&hash_input);

        let disbursement = Disbursement {
            id: disbursement_id,
            campaign_id,
            recipient,
            amount,
            milestone,
            status: DisbursementStatus::Pending,
            created_at: env.ledger().timestamp(),
            executed_at: None,
        };

        env.storage().persistent().set(&DataKey::Disbursement(disbursement_id), &disbursement);
        Ok(disbursement_id)
    }

    /// Execute approved disbursement
    pub fn execute_disbursement(
        env: Env,
        disbursement_id: BytesN<32>,
    ) -> Result<(), SaviaError> {
        let mut disbursement: Disbursement = env.storage().persistent().get(&DataKey::Disbursement(disbursement_id))
            .ok_or(SaviaError::DisbursementNotFound)?;

        if disbursement.status != DisbursementStatus::Approved {
            return Err(SaviaError::NotApproved);
        }

        disbursement.status = DisbursementStatus::Executed;
        disbursement.executed_at = Some(env.ledger().timestamp());

        env.storage().persistent().set(&DataKey::Disbursement(disbursement_id), &disbursement);
        Ok(())
    }

    /// Get disbursement details
    pub fn get_disbursement(env: Env, disbursement_id: BytesN<32>) -> Option<Disbursement> {
        env.storage().persistent().get(&DataKey::Disbursement(disbursement_id))
    }

    /// Helper function to determine badge type based on amount
    fn get_badge_type(env: &Env, amount: u64) -> String {
        if amount < 1000 {
            String::from_str(env, "Bronze Supporter")
        } else if amount < 5000 {
            String::from_str(env, "Silver Supporter")
        } else if amount < 10000 {
            String::from_str(env, "Gold Supporter")
        } else if amount < 50000 {
            String::from_str(env, "Platinum Supporter")
        } else {
            String::from_str(env, "Diamond Supporter")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    #[test]
    fn test_initialize_contract() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);

        let result = client.initialize(&200);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_campaign() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);

        client.initialize(&200);

        let beneficiary = Address::generate(&env);
        let result = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        );

        assert!(result.is_ok());
        let campaign_id = result.unwrap();
        let campaign = client.get_campaign(&campaign_id);
        assert!(campaign.is_some());
    }

    #[test]
    fn test_donation_flow() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);

        client.initialize(&200);

        let beneficiary = Address::generate(&env);
        let donor = Address::generate(&env);

        // Create campaign
        let campaign_id = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        ).unwrap();

        // Make donation
        let donation_id = client.donate(
            &campaign_id,
            &donor,
            &1000,
            &false,
            &true,
        ).unwrap();

        // Verify donation
        let donation = client.get_donation(&donation_id);
        assert!(donation.is_some());
        assert_eq!(donation.unwrap().amount, 980); // 1000 - 2% fee
    }
}